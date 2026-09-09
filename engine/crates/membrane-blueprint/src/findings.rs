//! Deterministic, generation-bound Blueprint findings.
//!
//! This seam consumes only moduleSurface facts already persisted on a loaded
//! Generation. It never scans, reparses, invokes a process, or consults a
//! network service.

use crate::api::{BlueprintError, BlueprintRequest, RequestContext};
use crate::export::{build_evidence_pack, findings_to_sarif};
use crate::store::Generation;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

const MAX_STAR_DEPTH: usize = 8;
const MAX_BASELINE_NAME: usize = 80;

pub fn execute_findings(
    generation: &Generation,
    request: &BlueprintRequest,
    context: &RequestContext,
    state_dir: &Path,
) -> Result<Value, BlueprintError> {
    context.check()?;
    let generation_id = generation.generation_id().unwrap_or("");
    if let Some(expected) = request.generation.as_deref().or_else(|| request.input.get("generation").and_then(Value::as_str)) {
        if expected != generation_id { return Err(BlueprintError::generation_mismatch(expected, generation_id)); }
    }
    let method = request.method.as_str();
    if method == "findings.baseline.list" { return baseline_list(generation, request, context, state_dir); }
    let bundle = detect(generation, context)?;
    let stale = request.input.get("stale").and_then(Value::as_bool).unwrap_or(false);
    if stale && request.input.get("allowStale").and_then(Value::as_bool) == Some(false) {
        return Err(BlueprintError::new("stale_blocked", "findings are stale; pass allowStale to accept known-stale evidence"));
    }
    match method {
        "findings.get" => findings_get(generation, request, context, bundle, stale, state_dir),
        "findings.explain" => findings_explain(generation, request, context, bundle, stale),
        "findings.evidence_pack" => findings_pack(generation, request, context, bundle, stale),
        "findings.baseline.capture" => baseline_capture(generation, request, context, bundle, stale, state_dir),
        "findings.sarif" => findings_sarif(generation, request, context, bundle, stale),
        _ => Err(BlueprintError::invalid(format!("unsupported findings operation: {method}"))),
    }
}

fn detect(generation: &Generation, context: &RequestContext) -> Result<Value, BlueprintError> {
    let id = generation.generation_id().unwrap_or("").to_owned();
    let mut surfaces: BTreeMap<String, Value> = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    let mut omissions = Vec::new();
    let mut scanned = BTreeSet::new();
    for node in &generation.nodes {
        context.check()?;
        if node.get("kind").and_then(Value::as_str) != Some("file") { continue; }
        let Some(path) = node.get("path").and_then(Value::as_str).map(norm) else { continue };
        scanned.insert(path.clone());
        let evidence = node.get("evidence").and_then(Value::as_array).and_then(|v| v.first());
        if let Some(hash) = evidence.and_then(|e| e.get("contentHash")).and_then(Value::as_str) { hashes.insert(path.clone(), hash.to_owned()); }
        let surface = evidence.and_then(|e| e.get("moduleSurface")).cloned().unwrap_or_else(|| json!({"path":path,"parseStatus":"unsupported","exports":[],"starReexports":[],"requests":[],"open":[]}));
        match surface.get("parseStatus").and_then(Value::as_str) {
            Some("ok") => { surfaces.insert(path, surface); }
            Some("failed") => omissions.push(omission(&path, "parse_failed", None, None, None)),
            _ => omissions.push(omission(&path, "unsupported_language", None, None, None)),
        }
    }
    let mut findings = Vec::new();
    let mut cache: HashMap<String, Option<BTreeSet<String>>> = HashMap::new();
    let paths: Vec<String> = surfaces.keys().cloned().collect();
    for path in paths {
        context.check()?;
        let requests = surfaces.get(&path).and_then(|s| s.get("requests")).and_then(Value::as_array).cloned().unwrap_or_default();
        for req in requests {
            context.check()?;
            let spec = req.get("specifier").and_then(Value::as_str).unwrap_or("");
            let line = req.get("line").and_then(Value::as_u64).unwrap_or(1);
            let name = req.get("name").and_then(Value::as_str).unwrap_or("");
            if !spec.starts_with('.') { omissions.push(omission(&path, "package_specifier", Some(spec), None, Some(line))); continue; }
            let candidates = resolve_candidates(&path, spec, &scanned);
            if candidates.len() > 1 { omissions.push(omission(&path, "resolution_ambiguous", Some(spec), Some(format!("matches {} files", candidates.len())), Some(line))); continue; }
            let Some(target) = candidates.first() else { findings.push(make_finding("BP002", &path, line, "", spec, &id, vec![path.clone()], &hashes, "resolves to no file in the repository")); continue; };
            let Some(surface) = surfaces.get(target) else {
                let reason = generation_surface_status(generation, target).unwrap_or("outside_scanned_set");
                omissions.push(omission(&path, reason, Some(spec), Some(target.clone()), Some(line)));
                continue;
            };
            let names = effective_exports(target, &surfaces, &scanned, &mut cache, &mut HashSet::new(), 0, &mut omissions);
            let Some(names) = names else { continue; };
            if names.contains(name) { continue; }
            let rule = if req.get("kind").and_then(Value::as_str) == Some("reexport") { "BP003" } else { "BP001" };
            let available = names.iter().take(6).cloned().collect::<Vec<_>>().join(", ");
            let suffix = if names.len() > 6 { format!(", +{} more", names.len()-6) } else { String::new() };
            let verb = if rule == "BP003" { "re-exports" } else { "imports" };
            let msg = if names.is_empty() { format!("{verb} {{ {name} }} from \"{spec}\" — that module exports nothing") } else { format!("{verb} {{ {name} }} from \"{spec}\" — that module exports {{ {available}{suffix} }} only") };
            findings.push(make_finding(rule, &path, line, name, spec, &id, vec![path.clone(), target.clone()], &hashes, &msg));
            let _ = surface;
        }
    }
    findings.sort_by(|a,b| key(a).cmp(&key(b)));
    Ok(json!({"schemaVersion":1,"kind":"findings-bundle","generationId":id,"findings":findings,"omissions":omissions,"coverage":{"filesScanned":scanned.len(),"filesParsed":surfaces.len(),"surfacesClosed":surfaces.values().filter(|s| s.get("open").and_then(Value::as_array).map(|a|a.is_empty()).unwrap_or(false)).count(),"omissionCount":omissions.len()},"perFileContentHashes":hashes}))
}

fn effective_exports(path: &str, surfaces: &BTreeMap<String, Value>, scanned: &BTreeSet<String>, cache: &mut HashMap<String, Option<BTreeSet<String>>>, seen: &mut HashSet<String>, depth: usize, omissions: &mut Vec<Value>) -> Option<BTreeSet<String>> {
    if let Some(v) = cache.get(path) { return v.clone(); }
    if depth >= MAX_STAR_DEPTH { omissions.push(omission(path, "star_depth_exceeded", None, None, None)); cache.insert(path.into(), None); return None; }
    if !seen.insert(path.into()) { omissions.push(omission(path, "star_cycle", None, None, None)); return None; }
    let surface = surfaces.get(path)?;
    if surface.get("open").and_then(Value::as_array).map(|a|!a.is_empty()).unwrap_or(false) { omissions.push(omission(path, "open_export_surface", None, None, None)); cache.insert(path.into(), None); return None; }
    let mut out = BTreeSet::new();
    if let Some(exports) = surface.get("exports").and_then(Value::as_array) { for x in exports { if let Some(n)=x.get("name").and_then(Value::as_str) { out.insert(n.into()); } } }
    if let Some(stars) = surface.get("starReexports").and_then(Value::as_array) { for star in stars { let spec=star.get("specifier").and_then(Value::as_str).unwrap_or(""); if !spec.starts_with('.') { omissions.push(omission(path,"open_export_surface",Some(spec),Some("unfollowable_star".into()),star.get("line").and_then(Value::as_u64))); return None; } let cs=resolve_candidates(path,spec,scanned); if cs.len()!=1 { omissions.push(omission(path,if cs.len()>1{"resolution_ambiguous"}else{"open_export_surface"},Some(spec),None,star.get("line").and_then(Value::as_u64))); return None; } let mut next=seen.clone(); let Some(inner)=effective_exports(&cs[0],surfaces,scanned,cache,&mut next,depth+1,omissions) else { return None }; for n in inner { if n != "default" { out.insert(n); } } } }
    cache.insert(path.into(), Some(out.clone())); Some(out)
}

fn findings_get(g: &Generation, req: &BlueprintRequest, ctx: &RequestContext, bundle: Value, stale: bool, state: &Path) -> Result<Value, BlueprintError> { let mut out=bundle; let prefixes=paths(req); if !prefixes.is_empty(){filter_bundle(&mut out,&prefixes);} let delta=req.input.get("baselineGeneration").and_then(Value::as_str).map(|n| baseline_delta(ctx,state,n,&out).unwrap_or_else(|_|json!({"added":[],"persistent":[],"resolved":[],"changed":[],"unknown":true}))); out["kind"]=json!("findings.get"); out["root"]=json!(ctx.scope.repo_root); out["freshness"]=json!(if stale{"stale"}else{"current"}); out["delta"]=delta.unwrap_or(Value::Null); bound_response(&out,ctx)?; Ok(out) }
fn findings_explain(g: &Generation, req: &BlueprintRequest, ctx: &RequestContext, bundle: Value, stale: bool) -> Result<Value, BlueprintError> { ctx.check()?; let fp=req.input.get("fingerprint").and_then(Value::as_str).ok_or_else(||BlueprintError::missing("fingerprint"))?; let finding=bundle.get("findings").and_then(Value::as_array).and_then(|a|a.iter().find(|x|x.get("fingerprint").is_some_and(|v|v==fp))).cloned().ok_or_else(||BlueprintError::new("finding_not_found",format!("finding {fp} is not present")))?; let out=json!({"schemaVersion":1,"kind":"findings.explain","generationId":g.generation_id(),"freshness":if stale{"stale"}else{"current"},"finding":finding,"reasoning":{"ruleName":finding["ruleName"],"description":finding["ruleDescription"],"message":finding["message"],"remediation":finding["remediation"],"precisionTier":finding["precisionTier"],"confidenceTier":finding["confidenceTier"]},"evidence":evidence_rows(&finding,bundle.get("perFileContentHashes")),"omissions":if stale{json!([{"reason":"stale_generation"}])}else{json!([])}}); bound_response(&out,ctx)?; Ok(out) }
fn findings_pack(g: &Generation, req: &BlueprintRequest, ctx: &RequestContext, bundle: Value, _stale: bool) -> Result<Value, BlueprintError> { ctx.check()?; let fps=req.input.get("fingerprints").and_then(Value::as_array).ok_or_else(||BlueprintError::missing("fingerprints"))?; if fps.is_empty(){return Err(BlueprintError::new("finding_selection_empty","at least one fingerprint is required"))} if fps.len()>100{return Err(BlueprintError::oversized("finding_selection"))} let all=bundle.get("findings").and_then(Value::as_array).cloned().unwrap_or_default(); let mut selected=Vec::new(); for fp in fps { ctx.check()?; let s=fp.as_str().unwrap_or(""); selected.push(all.iter().find(|f|f["fingerprint"]==s).cloned().ok_or_else(||BlueprintError::new("finding_not_found",format!("finding {s} is not present")))?); } let pack=build_evidence_pack(req.repo_id.as_deref(),g.generation_id().unwrap_or(""),&selected)?; let out=json!({"schemaVersion":1,"kind":"findings.evidence_pack","generationId":g.generation_id(),"pack":pack}); bound_response(&out,ctx)?; Ok(out) }
fn findings_sarif(g:&Generation,req:&BlueprintRequest,ctx:&RequestContext,mut bundle:Value,stale:bool)->Result<Value,BlueprintError>{let p=paths(req);if !p.is_empty(){filter_bundle(&mut bundle,&p)} let fs=bundle["findings"].as_array().cloned().unwrap_or_default();let sarif=findings_to_sarif(&fs,req.input.get("toolVersion").and_then(Value::as_str).unwrap_or("native-rust"));bound_response(&sarif,ctx)?;Ok(json!({"schemaVersion":1,"kind":"findings.sarif","generationId":g.generation_id(),"freshness":if stale{"stale"}else{"current"},"findingCount":fs.len(),"omissions":bundle["omissions"],"sarif":sarif}))}

fn baseline_capture(g:&Generation,req:&BlueprintRequest,ctx:&RequestContext,bundle:Value,stale:bool,state:&Path)->Result<Value,BlueprintError>{ctx.check()?;let name=safe_name(req.input.get("name").and_then(Value::as_str).ok_or_else(||BlueprintError::missing("name"))?)?;fs::create_dir_all(state).map_err(|e|BlueprintError::new("baseline_io",e.to_string()))?;ctx.check()?;let source_digest=digest_value(bundle.get("perFileContentHashes").unwrap_or(&Value::Null))?;let record=json!({"schemaVersion":1,"kind":"findings-baseline","name":name,"generationId":g.generation_id(),"sourceDigest":source_digest,"detectorVersion":"native-rust-findings-v1","config":{"maxStarDepth":MAX_STAR_DEPTH},"findingCount":bundle["findings"].as_array().map(|a|a.len()).unwrap_or(0),"coverage":bundle["coverage"],"findings":bundle["findings"]});let path=state.join(format!("{name}.json"));let tmp=state.join(format!(".{name}.{}.tmp",std::process::id()));let mut file=File::create(&tmp).map_err(|e|BlueprintError::new("baseline_io",e.to_string()))?;let bytes=serde_json::to_vec(&record).map_err(|e|BlueprintError::malformed(e.to_string()))?;use std::io::Write;file.write_all(&bytes).map_err(|e|BlueprintError::new("baseline_io",e.to_string()))?;file.sync_all().map_err(|e|BlueprintError::new("baseline_io",e.to_string()))?;ctx.check()?;replace_atomic(&tmp,&path)?;let out=json!({"schemaVersion":1,"kind":"findings.baseline.capture","name":name,"generationId":g.generation_id(),"findingCount":record["findingCount"],"path":path,"freshness":if stale{"stale"}else{"current"}});bound_response(&out,ctx)?;Ok(out)}
fn baseline_list(g:&Generation,_:&BlueprintRequest,ctx:&RequestContext,state:&Path)->Result<Value,BlueprintError>{ctx.check()?;let mut rows=Vec::new();if let Ok(entries)=fs::read_dir(state){for e in entries.flatten(){ctx.check()?;if e.path().extension().and_then(|x|x.to_str())!=Some("json"){continue}if let Ok(v)=fs::read(e.path()).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()).ok_or(()){if v.get("kind").and_then(Value::as_str)==Some("findings-baseline"){rows.push(json!({"name":v["name"],"generationId":v["generationId"],"findingCount":v["findingCount"],"path":e.path()}));}}}}rows.sort_by(|a,b|a["name"].as_str().cmp(&b["name"].as_str()));let out=json!({"schemaVersion":1,"kind":"findings.baseline.list","generationId":g.generation_id(),"baselines":rows});bound_response(&out,ctx)?;Ok(out)}

fn make_finding(rule:&str,path:&str,line:u64,name:&str,spec:&str,generation:&str,evidence:Vec<String>,hashes:&BTreeMap<String,String>,message:&str)->Value{let fp=hex::encode(&Sha256::digest(format!("{rule} {path} {name} {spec}").as_bytes())[..8]);let rows=evidence.iter().enumerate().map(|(i,p)|json!({"path":p,"startLine":if i==0{json!(line)}else{Value::Null},"endLine":if i==0{json!(line)}else{Value::Null},"contentHash":hashes.get(p).cloned().map(Value::String).unwrap_or(Value::Null)})).collect::<Vec<_>>();json!({"ruleId":rule,"ruleName":match rule{"BP001"=>"import-binding-not-exported","BP002"=>"module-not-found",_=>"reexport-binding-not-exported"},"ruleDescription":match rule{"BP001"=>"An imported name is not exported by the module the specifier resolves to.","BP002"=>"A repository-relative import specifier resolves to no file in the repository.",_=>"A re-export names a binding the target module does not export."},"severity":"error","class":"block","confidenceTier":"EXACT_RESOLUTION","precisionTier":"AST","path":path,"startLine":line,"endLine":line,"message":message,"name":if name.is_empty(){Value::Null}else{json!(name)},"specifier":spec,"evidencePath":evidence,"evidence":rows,"perFileContentHashes":hashes,"generationId":generation,"fingerprint":fp,"remediation":"Correct the specifier or export the missing binding.","helpUri":Value::Null})}
fn generation_surface_status(g:&Generation,target:&str)->Option<&'static str>{for n in &g.nodes{if n.get("path").and_then(Value::as_str).map(norm).as_deref()==Some(target){let s=n.get("evidence").and_then(Value::as_array).and_then(|a|a.first()).and_then(|e|e.get("moduleSurface")).and_then(|s|s.get("parseStatus")).and_then(Value::as_str);return Some(if s==Some("failed"){"parse_failed"}else{"unsupported_language"});}}None}
fn omission(path:&str,reason:&str,spec:Option<&str>,detail:Option<String>,line:Option<u64>)->Value{json!({"path":path,"reason":reason,"detail":detail,"specifier":spec,"line":line})}
fn resolve_candidates(from:&str,spec:&str,files:&BTreeSet<String>)->Vec<String>{let base=norm(Path::new(from).parent().unwrap_or(Path::new(".")).join(spec).to_string_lossy().as_ref());let mut out=Vec::new();let candidates=vec![base.clone(),format!("{base}.js"),format!("{base}.jsx"),format!("{base}.ts"),format!("{base}.tsx"),format!("{base}.mjs"),format!("{base}.cjs"),format!("{base}/index.js"),format!("{base}/index.ts")];for p in candidates{if files.contains(&p){out.push(p)}}out.sort();out.dedup();out}
fn norm(v:&str)->String{v.replace('\\',"/").trim_start_matches("./").to_string()}
fn key(v:&Value)->String{format!("{}:{}:{}",v["path"].as_str().unwrap_or(""),v["startLine"].as_u64().unwrap_or(0),v["ruleId"].as_str().unwrap_or(""))}
fn paths(req:&BlueprintRequest)->Vec<String>{req.input.get("paths").and_then(Value::as_array).map(|a|a.iter().filter_map(Value::as_str).map(norm).collect()).unwrap_or_default()}
fn filter_bundle(v:&mut Value,p:&[String]){if let Some(a)=v["findings"].as_array().cloned(){v["findings"]=json!(a.into_iter().filter(|x|p.iter().any(|q|x["path"].as_str().unwrap_or("")==*q||x["path"].as_str().unwrap_or("").starts_with(&(q.clone()+"/")))).collect::<Vec<_>>())} }
fn evidence_rows(f:&Value,h:Option<&Value>)->Value{json!(f["evidencePath"].as_array().map(|a|a.iter().enumerate().map(|(i,p)|json!({"path":p,"startLine":if i==0{f["startLine"].clone()}else{Value::Null},"endLine":if i==0{f["endLine"].clone()}else{Value::Null},"contentHash":h.and_then(|x|x.get(p.as_str().unwrap_or(""))).cloned().unwrap_or(Value::Null)})).collect::<Vec<_>>()).unwrap_or_default())}
fn safe_name(s:&str)->Result<String,BlueprintError>{if s.is_empty()||s.len()>MAX_BASELINE_NAME||!s.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'||b==b'.'){return Err(BlueprintError::invalid("baseline name must be a safe filename"))}Ok(s.into())}
fn baseline_delta(ctx:&RequestContext,state:&Path,name:&str,current:&Value)->Result<Value,BlueprintError>{ctx.check()?;let n=safe_name(name)?;let b=serde_json::from_slice::<Value>(&fs::read(state.join(format!("{n}.json"))).map_err(|_|BlueprintError::new("baseline_unknown","baseline not found"))?).map_err(|_|BlueprintError::new("baseline_corrupt","baseline is corrupt"))?;ctx.check()?;let old=b["findings"].as_array().cloned().unwrap_or_default();let now=current["findings"].as_array().cloned().unwrap_or_default();let oldfp:HashMap<_,_>=old.iter().filter_map(|x|x["fingerprint"].as_str().map(|f|(f,x))).collect();let nowfp:HashMap<_,_>=now.iter().filter_map(|x|x["fingerprint"].as_str().map(|f|(f,x))).collect();let identity=|x:&Value|format!("{}|{}|{}",x["ruleId"].as_str().unwrap_or(""),x["path"].as_str().unwrap_or(""),x["name"].as_str().unwrap_or(""));let mut oldid=HashMap::new();for x in &old{oldid.insert(identity(x),x);}let mut nowid=HashMap::new();for x in &now{nowid.insert(identity(x),x);}let added=now.iter().filter(|x|!oldfp.contains_key(x["fingerprint"].as_str().unwrap_or(""))&&!oldid.contains_key(&identity(x))).cloned().collect::<Vec<_>>();let resolved=old.iter().filter(|x|!nowfp.contains_key(x["fingerprint"].as_str().unwrap_or(""))&&!nowid.contains_key(&identity(x))).cloned().collect::<Vec<_>>();let changed=now.iter().filter(|x|!oldfp.contains_key(x["fingerprint"].as_str().unwrap_or(""))&&oldid.get(&identity(x)).is_some()).cloned().collect::<Vec<_>>();let persistent=now.iter().filter(|x|oldfp.contains_key(x["fingerprint"].as_str().unwrap_or(""))).cloned().collect::<Vec<_>>();Ok(json!({"added":added,"persistent":persistent,"resolved":resolved,"changed":changed,"unknown":false}))}
fn replace_atomic(tmp:&Path,target:&Path)->Result<(),BlueprintError>{if !target.exists(){return fs::rename(tmp,target).map_err(|e|BlueprintError::new("baseline_io",e.to_string()));}let backup=target.with_extension(format!("bak.{}",std::process::id()));fs::rename(target,&backup).map_err(|e|BlueprintError::new("baseline_io",e.to_string()))?;match fs::rename(tmp,target){Ok(())=>{let _=fs::remove_file(backup);Ok(())},Err(e)=>{let _=fs::rename(&backup,target);Err(BlueprintError::new("baseline_io",e.to_string()))}}}
fn digest_value(value:&Value)->Result<String,BlueprintError>{let bytes=serde_json::to_vec(value).map_err(|e|BlueprintError::malformed(e.to_string()))?;Ok(hex::encode(Sha256::digest(bytes)))}
fn bound_response(v:&Value,c:&RequestContext)->Result<(),BlueprintError>{if serde_json::to_vec(v).map_err(|e|BlueprintError::malformed(e.to_string()))?.len()>c.bounds.max_response_bytes{Err(BlueprintError::oversized("response_bytes"))}else{Ok(())}}
