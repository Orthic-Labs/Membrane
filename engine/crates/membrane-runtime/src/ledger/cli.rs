//! Stateless Ledger command adapter. The active daemon owns every operation.
use clap::Subcommand;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(crate) enum LedgerCmd {
    Outline {
        #[arg(long)] repo: PathBuf,
        #[arg(long)] path: PathBuf,
        #[arg(long)] json: bool,
        #[arg(long)] continuation_cursor: Option<String>,
    },
    Read {
        #[arg(long)] repo: PathBuf,
        #[arg(long)] source_ref: String,
        #[arg(long)] anchor: String,
        #[arg(long)] expected_hash: String,
        #[arg(long)] doc_id: Option<String>,
        #[arg(long)] node_id: Option<String>,
        #[arg(long)] expected_revision: Option<String>,
        #[arg(long)] expected_span_hash: Option<String>,
        #[arg(long)] ledger_generation: Option<i64>,
        #[arg(long)] ledger_ticket: Option<String>,
        #[arg(long)] continuation_cursor: Option<String>,
        #[arg(long,default_value_t=12000)] max_bytes: usize,
    },
    Sync { #[arg(long)] repo: PathBuf },
    Recall {
        #[arg(long)] repo: PathBuf,
        query: String,
        #[arg(short,default_value_t=6)] k: usize,
    },
    Literal {
        #[arg(long)] repo: PathBuf,
        query: String,
        #[arg(short,default_value_t=6)] k: usize,
    },
    Status { #[arg(long)] repo: PathBuf },
    Activate {
        #[arg(long)] repo: PathBuf,
        #[arg(value_parser=["legacy_scan","shadow","ledger_fts"])] mode: String,
    },
    Erase {
        #[arg(long)] repo: PathBuf,
        #[arg(long)] doc_id: String,
        #[arg(long)] expected_hash: String,
    },
    Backlinks {
        #[arg(long)] repo: PathBuf,
        #[arg(long)] doc_id: String,
        #[arg(long)] node_id: Option<String>,
        #[arg(long,default_value_t=64)] limit: usize,
    },
    Drift {
        #[arg(long)] repo: PathBuf,
        #[arg(long)] doc_id: String,
        #[arg(long)] from_manifest: String,
        #[arg(long)] to_manifest: String,
    },
    Manifests {
        #[arg(long)] repo: PathBuf,
        #[arg(long)] doc_id: String,
    },
}

fn arguments(repo:&Path, operation:&str)->Result<Value,String> {
    if !repo.is_absolute() { return Err("ledger --repo must be an explicit absolute enrolled root".into()); }
    let identity = membrane_federation::root::canonical_repository_id(repo);
    let caller = super::service::Caller::enrolled(repo,&identity)?;
    Ok(json!({"operation":operation,"repository":caller.repository_id,"caller":caller.envelope()}))
}
fn optional(value:&mut Value, key:&str, field:&Option<String>) {
    if let Some(field)=field {value[key]=json!(field);}
}

pub(crate) fn run(command:&LedgerCmd)->Result<(),String> {
    // Discover the active installation before any index/source work. This
    // client never opens a Ledger DB, starts a daemon, or chooses cwd as scope.
    let client = crate::mcp_executor::active_hub_client()?;
    let (tool,args) = match command {
        LedgerCmd::Outline{repo,path,json:as_json,continuation_cursor} => {
            if !as_json {return Err("ledger outline requires --json".into());}
            let mut args=arguments(repo,"outline")?;
            let path = if path.is_absolute() { path.strip_prefix(repo).map_err(|_|"ledger path outside repository")? } else {path.as_path()};
            args["path"]=json!(path.to_str().ok_or("ledger path encoding unsupported")?.replace('\\',"/"));
            optional(&mut args,"continuationCursor",continuation_cursor);
            ("membrane_ledger",args)
        },
        LedgerCmd::Read{repo,source_ref,anchor,expected_hash,doc_id,node_id,expected_revision,
            expected_span_hash,ledger_generation,ledger_ticket,continuation_cursor,max_bytes} => {
            let mut args=arguments(repo,"read")?;
            args.as_object_mut().unwrap().remove("operation");
            args["sourceRef"]=json!(source_ref);args["anchorId"]=json!(anchor);
            args["expectedContentHash"]=json!(expected_hash);args["maxBytes"]=json!(max_bytes);
            optional(&mut args,"docId",doc_id);optional(&mut args,"nodeId",node_id);
            optional(&mut args,"expectedRevision",expected_revision);
            optional(&mut args,"expectedSpanHash",expected_span_hash);
            optional(&mut args,"ledgerTicket",ledger_ticket);
            optional(&mut args,"continuationCursor",continuation_cursor);
            if let Some(generation)=ledger_generation {args["ledgerGeneration"]=json!(generation);}
            ("membrane_source_read",args)
        },
        LedgerCmd::Sync{repo} => ("membrane_ledger",arguments(repo,"sync")?),
        LedgerCmd::Status{repo} => ("membrane_ledger",arguments(repo,"status")?),
        LedgerCmd::Recall{repo,query,k}|LedgerCmd::Literal{repo,query,k} => {
            let mut args=arguments(repo,if matches!(command,LedgerCmd::Literal{..}){"literal"}else{"recall"})?;
            args["query"]=json!(query);args["k"]=json!(k);("membrane_ledger",args)
        },
        LedgerCmd::Activate{repo,mode} => {
            let mut args=arguments(repo,"activate")?;args["mode"]=json!(mode);("membrane_ledger",args)
        },
        LedgerCmd::Erase{repo,doc_id,expected_hash} => {
            let mut args=arguments(repo,"erase")?;args["docId"]=json!(doc_id);
            args["expectedContentHash"]=json!(expected_hash);("membrane_ledger",args)
        },
        LedgerCmd::Backlinks{repo,doc_id,node_id,limit} => {
            let mut args=arguments(repo,"backlinks")?;args["docId"]=json!(doc_id);
            args["limit"]=json!(limit);optional(&mut args,"nodeId",node_id);("membrane_ledger",args)
        },
        LedgerCmd::Drift{repo,doc_id,from_manifest,to_manifest} => {
            let mut args=arguments(repo,"drift")?;args["docId"]=json!(doc_id);
            args["fromManifest"]=json!(from_manifest);args["toManifest"]=json!(to_manifest);("membrane_ledger",args)
        },
        LedgerCmd::Manifests{repo,doc_id} => {
            let mut args=arguments(repo,"manifests")?;args["docId"]=json!(doc_id);("membrane_ledger",args)
        },
    };
    let response=client.execute(tool,&args);
    println!("{}",serde_json::to_string(&response).map_err(|e|e.to_string())?);
    if response.pointer("/result/kind").and_then(Value::as_str)!=Some("success") {
        return Err(response.pointer("/result/code").and_then(Value::as_str).unwrap_or("ledger_operation_failed").into());
    }
    Ok(())
}
