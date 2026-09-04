//! Read-only, source-scoped backlinks and named-manifest structural diagnostics.
//!
//! A manifest is a bounded rebuildable projection, not a change feed or durable
//! truth. Missing baselines are explicit; ambiguous identical blocks never
//! become guessed moves. Reference counts confer no authority or rank boost.
use super::{index, limits::WorkBudget, resolve, LedgerDb};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_document_manifests (
 manifest_id TEXT PRIMARY KEY, repository_root TEXT NOT NULL, doc_id TEXT NOT NULL,
 source_revision TEXT NOT NULL, content_hash TEXT NOT NULL, projection_version TEXT NOT NULL,
 nodes_json TEXT NOT NULL, ledger_generation INTEGER NOT NULL, created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS ledger_manifest_document ON ledger_document_manifests(repository_root,doc_id,created_at_ms);
"#;
const MANIFEST_VERSION: &str = "ledger.manifest.v1";
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const RETAINED_MANIFESTS: i64 = 4;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
struct ManifestNode {
    node_id: String, anchor: String, parent: Option<String>, kind: String,
    start: i64, end: i64, span_hash: String,
}
fn manifest_id(root:&str,doc:&str,hash:&str,projection:&str,nodes:&str)->String {
    format!("ledger.manifest:{}",resolve::digest(format!("{MANIFEST_VERSION}\0{root}\0{doc}\0{hash}\0{projection}\0{nodes}").as_bytes()))
}

/// Called inside the exact transaction that publishes this document's nodes.
pub(crate) fn record_manifest_tx(tx:&Transaction<'_>,doc_id:&str)->rusqlite::Result<()> {
    tx.execute_batch(SCHEMA)?;
    let (root,revision,hash,generation):(String,String,String,i64)=tx.query_row(
        "SELECT repository_root,revision,content_hash,index_generation FROM ledger_doc_artifacts WHERE doc_id=?1",
        [doc_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let nodes={
        let mut statement=tx.prepare("SELECT node_id,anchor_id,parent_id,node_kind,source_start_byte,source_end_byte,span_hash
            FROM ledger_nodes WHERE doc_id=?1 AND ledger_generation=?2 ORDER BY ordinal,node_id LIMIT 32769")?;
        let rows=statement.query_map(params![doc_id,generation],|r|Ok(ManifestNode {
            node_id:r.get(0)?,anchor:r.get(1)?,parent:r.get(2)?,kind:r.get(3)?,start:r.get(4)?,end:r.get(5)?,span_hash:r.get(6)?,
        }))?.collect::<Result<Vec<_>,_>>()?;
        rows
    };
    let nodes=serde_json::to_string(&nodes).map_err(|e|rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    if nodes.len()>MAX_MANIFEST_BYTES {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other("ledger_manifest_budget_exhausted"))));
    }
    let id=manifest_id(&root,doc_id,&hash,index::PROJECTION_SCHEMA_VERSION,&nodes);
    tx.execute("INSERT OR IGNORE INTO ledger_document_manifests VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![id,root,doc_id,revision,hash,index::PROJECTION_SCHEMA_VERSION,nodes,generation,crate::time::now_millis() as i64])?;
    tx.execute("DELETE FROM ledger_document_manifests WHERE manifest_id IN (
        SELECT manifest_id FROM ledger_document_manifests WHERE repository_root=?1 AND doc_id=?2
        ORDER BY created_at_ms DESC,manifest_id LIMIT -1 OFFSET ?3)",params![root,doc_id,RETAINED_MANIFESTS])?;
    Ok(())
}

fn authorize_document(db:&LedgerDb,root:&str,doc:&str,budget:&WorkBudget)->Result<resolve::Source,String> {
    let path:String=db.lock().query_row("SELECT path FROM ledger_doc_artifacts WHERE repository_root=?1 AND doc_id=?2",
        params![root,doc],|r|r.get(0)).optional().map_err(|e|e.to_string())?.ok_or("ledger_source_missing")?;
    super::service::permitted_path(db,root,&path,budget)?;
    let source=resolve::load_source(db,root,doc).map_err(|e|format!("ledger_source_{e}"))?;
    budget.charge_bytes(source.markdown.len())?;
    Ok(source)
}

pub(crate) fn manifests(db:&LedgerDb,root:&str,doc:&str,budget:&WorkBudget)->Result<Value,String> {
    authorize_document(db,root,doc,budget)?;
    let conn=db.lock();
    let mut statement=conn.prepare("SELECT manifest_id,source_revision,content_hash,projection_version,ledger_generation
        FROM ledger_document_manifests WHERE repository_root=?1 AND doc_id=?2 ORDER BY created_at_ms DESC,manifest_id LIMIT 4")
        .map_err(|e|e.to_string())?;
    let rows=statement.query_map(params![root,doc],|r|Ok(json!({"manifestId":r.get::<_,String>(0)?,
        "sourceRevision":r.get::<_,String>(1)?,"contentHash":r.get::<_,String>(2)?,
        "projectionVersion":r.get::<_,String>(3)?,"generation":r.get::<_,i64>(4)?})))
        .map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    Ok(json!({"schemaVersion":1,"docId":doc,"manifests":rows,"retainedMaximum":RETAINED_MANIFESTS,
        "historyComplete":false,"missingBaselineOutcome":"ledger_baseline_unavailable"}))
}

fn load_manifest(db:&LedgerDb,root:&str,doc:&str,id:&str)->Result<(String,Vec<ManifestNode>),String> {
    let (hash,projection,nodes):(String,String,String)=db.lock().query_row(
        "SELECT content_hash,projection_version,nodes_json FROM ledger_document_manifests
         WHERE repository_root=?1 AND doc_id=?2 AND manifest_id=?3 AND length(CAST(nodes_json AS BLOB))<=1048576",
        params![root,doc,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()
        .map_err(|e|e.to_string())?.ok_or("ledger_baseline_unavailable")?;
    if manifest_id(root,doc,&hash,&projection,&nodes)!=id {return Err("ledger_manifest_corrupt".into());}
    let nodes=serde_json::from_str(&nodes).map_err(|_|"ledger_manifest_corrupt")?;
    Ok((projection,nodes))
}

fn difference(before:&[ManifestNode],after:&[ManifestNode])->Value {
    let mut old:BTreeMap<(String,String),Vec<&ManifestNode>>=BTreeMap::new();
    let mut new:BTreeMap<(String,String),Vec<&ManifestNode>>=BTreeMap::new();
    for node in before {old.entry((node.kind.clone(),node.span_hash.clone())).or_default().push(node);}
    for node in after {new.entry((node.kind.clone(),node.span_hash.clone())).or_default().push(node);}
    let mut unchanged=0usize;
    let mut relocated=Vec::new();
    let mut removed=Vec::new();
    let mut added=Vec::new();
    let mut ambiguous=Vec::new();
    let keys=old.keys().chain(new.keys()).cloned().collect::<BTreeSet<_>>();
    for key in keys {
        let previous=old.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let current=new.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        match (previous,current) {
            ([a],[b]) => {
                if a.start==b.start && a.end==b.end && a.parent==b.parent && a.anchor==b.anchor {unchanged+=1;}
                else {relocated.push(json!({"before":a.node_id,"after":b.node_id,"kind":"position_or_context_changed","spanHash":a.span_hash}));}
            },
            ([],current) => added.extend(current.iter().map(|n|n.node_id.clone())),
            (previous,[]) => removed.extend(previous.iter().map(|n|n.node_id.clone())),
            _ => ambiguous.push(json!({"nodeKind":key.0,"spanHash":key.1,"beforeCount":previous.len(),"afterCount":current.len()})),
        }
    }
    json!({"unchangedNodes":unchanged,"relocated":relocated,"addedNodeIds":added,"removedNodeIds":removed,
        "ambiguousIdenticalSpans":ambiguous,"countsDescribe":"projected nodes, including nested containers",
        "semanticTruthChanged":false,"rankingEffect":"none"})
}

pub(crate) fn drift(db:&LedgerDb,root:&str,doc:&str,from:&str,to:&str,budget:&WorkBudget)->Result<Value,String> {
    authorize_document(db,root,doc,budget)?;
    let (old_version,before)=load_manifest(db,root,doc,from)?;
    let (new_version,after)=load_manifest(db,root,doc,to)?;
    if old_version!=new_version {return Err("ledger_manifest_version_incompatible".into());}
    budget.charge_bytes((before.len()+after.len()).saturating_mul(256))?;
    let mut value=difference(&before,&after);
    value["schemaVersion"]=json!(1);value["docId"]=json!(doc);
    value["fromManifest"]=json!(from);value["toManifest"]=json!(to);
    budget.check()?;
    Ok(value)
}

pub(crate) fn backlinks(db:&LedgerDb,root:&str,doc:&str,node:Option<&str>,limit:usize,budget:&WorkBudget)->Result<Value,String> {
    if limit==0 || limit>256 {return Err("ledger_limit_invalid".into());}
    let target=authorize_document(db,root,doc,budget)?;
    let target_span=if let Some(node)=node {
        Some(db.lock().query_row("SELECT span_hash FROM ledger_nodes WHERE doc_id=?1 AND node_id=?2 AND ledger_generation=?3",
            params![doc,node,target.generation],|r|r.get::<_,String>(0)).optional().map_err(|e|e.to_string())?.ok_or("ledger_node_missing")?)
    } else {None};
    let rows={
        let conn=db.lock();
        let mut statement=conn.prepare("SELECT l.source_doc_id,a.path,l.source_revision,l.source_span_hash,l.source_start_byte,
            l.source_end_byte,l.target_revision,l.target_content_hash,l.target_span_hash,l.link_kind
            FROM ledger_link_targets l JOIN ledger_doc_artifacts a ON a.doc_id=l.source_doc_id
            WHERE l.target_doc_id=?1 AND a.repository_root=?2 AND a.lifecycle_state='active' AND a.sensitivity='normal'
              AND l.ledger_generation=a.index_generation AND l.resolution_state='resolved'
              AND (?3 IS NULL OR l.target_span_hash=?3)
            ORDER BY a.path,l.source_start_byte LIMIT 4097").map_err(|e|e.to_string())?;
        let rows=statement.query_map(params![doc,root,target_span],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,
            r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,
            r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,Option<String>>(8)?,r.get::<_,String>(9)?)))
            .map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
        rows
    };
    let mut complete=rows.len()<=4096;
    let mut references=Vec::new();
    let mut sources=BTreeMap::new();
    let mut omissions=BTreeSet::new();
    for (source_id,path,revision,span,start,end,target_revision,target_hash,link_target_span,kind) in rows.into_iter().take(4096) {
        budget.visit()?;
        if super::service::permitted_path(db,root,&path,budget).is_err() {continue;}
        if !sources.contains_key(&source_id) {
            match resolve::load_source(db,root,&source_id) {
                Ok(source)=>{budget.charge_bytes(source.markdown.len())?;sources.insert(source_id.clone(),source);},
                Err(_)=>{complete=false;omissions.insert("source_unavailable");continue;},
            }
        }
        let source=&sources[&source_id];
        if revision!=source.revision || target_revision.as_deref()!=Some(&target.revision)
            || target_hash.as_deref()!=Some(&target.raw_hash) || start<0 || end<start {
            complete=false;omissions.insert("edge_stale");continue;
        }
        let Some(text)=source.markdown.get(start as usize..end as usize) else {complete=false;omissions.insert("span_stale");continue;};
        if resolve::digest(text.as_bytes())!=span {complete=false;omissions.insert("span_stale");continue;}
        if references.len()==limit {complete=false;omissions.insert("result_limit");break;}
        references.push(json!({"sourceDocId":source_id,"sourceRevision":revision,"sourceSpanHash":span,
            "sourceStartByte":start,"sourceEndByte":end,"linkKind":kind,"targetDocId":doc,
            "targetSpanHash":link_target_span,"generation":target.generation}));
    }
    authorize_document(db,root,doc,budget)?;
    Ok(json!({"schemaVersion":1,"docId":doc,"nodeId":node,"references":references,"complete":complete,
        "visibleCount":references.len(),"countKind":if complete{"exact_visible_generation"}else{"lower_bound"},
        "orphan":if complete{Some(references.is_empty())}else{None},"omissions":omissions,
        "authorityEffect":"none","rankingEffect":"none"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(id:&str,start:i64,hash:&str)->ManifestNode {ManifestNode{node_id:id.into(),anchor:"sec:test:1".into(),
        parent:None,kind:"paragraph".into(),start,end:start+4,span_hash:hash.into()}}
    #[test]
    fn identical_span_ties_do_not_become_guessed_moves() {
        let value=difference(&[node("a",0,"same"),node("b",5,"same")],&[node("c",9,"same")]);
        assert_eq!(value["ambiguousIdenticalSpans"].as_array().unwrap().len(),1);
        assert!(value["relocated"].as_array().unwrap().is_empty());
    }
    #[test]
    fn changed_position_reports_only_source_evidence_not_truth() {
        let value=difference(&[node("a",0,"same")],&[node("b",9,"same")]);
        assert_eq!(value["relocated"].as_array().unwrap().len(),1);
        assert_eq!(value["semanticTruthChanged"],false);
    }
}
