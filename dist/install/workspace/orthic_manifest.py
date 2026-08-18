"""Orthic Hub product manifest producer — pure stdlib, explicit paths only."""
from __future__ import annotations
import getpass, json, os, subprocess, tempfile
from pathlib import Path
SCHEMA_VERSION=1
PRODUCT_ID="membrane"
DISPLAY_NAME="Membrane"
ALLOWED_FIELDS={"schemaVersion","productId","displayName","productVersion","hubCompatRange","installRoot","serviceStart","serviceStop","statusEndpoint","icon"}
HUB_COMPAT_RANGE=">=0.1.11 <0.2.0"
ICON_REL=Path("install/assets/membrane-tab-icon.png")
WORKSPACE_ICON_REL=Path("membrane/install/assets/membrane-tab-icon.png")
SVC_REL=Path("tools/bin/crypt-service")
SVC_REL_WIN=Path("tools/bin/crypt-service.exe")
TOKEN_REL=Path("tools/.cache/memory/api-token")
def _is_inside(root:Path,target:Path)->bool:
    try: target.resolve().relative_to(root.resolve()); return True
    except ValueError: return False
def _svc_bin(root:Path,win:bool|None=None)->Path:
    if win is None: win=os.name=="nt"
    return root/(SVC_REL_WIN if win else SVC_REL)
def _icon(root:Path)->Path:
    for relative in (ICON_REL,WORKSPACE_ICON_REL):
        candidate=root/relative
        if candidate.is_file(): return candidate
    return root/ICON_REL
def _secure_file(path:Path)->None:
    os.chmod(path,0o600)
    if os.name=="nt":
        result=subprocess.run(
            ["icacls",str(path),"/inheritance:r","/grant:r",f"{getpass.getuser()}:(F)"],
            capture_output=True,text=True,
        )
        if result.returncode!=0: raise OSError(f"unable to secure manifest ACL: {result.stderr.strip()}")
def read_product_version(root:Path)->str:
    for c in (root/"engine/crates/membrane/Cargo.toml",root/"membrane/engine/crates/membrane/Cargo.toml"):
      if c.is_file():
        for l in c.read_text(encoding="utf-8").splitlines():
            s=l.strip()
            if s.startswith("version"):
                p=s.split("=",1)
                if len(p)==2:
                    v=p[1].strip().strip('"').strip("'")
                    if v: return v
    return "0.1.0"
def build_manifest_dict(root:Path,port:int,product_version:str,*,win:bool|None=None)->dict:
    root=Path(root)
    if not root.is_absolute(): raise ValueError(f"installRoot must be absolute: {root}")
    if not str(product_version).strip(): raise ValueError("productVersion must be nonempty")
    if not 1<=int(port)<=65535: raise ValueError("statusEndpoint.port must be within 1..=65535")
    svc=_svc_bin(root,win=win); icon=_icon(root); token=root/TOKEN_REL
    if not svc.is_file(): raise FileNotFoundError(f"crypt-service binary not found: {svc}")
    if not icon.is_file(): raise FileNotFoundError(f"icon not found: {icon}")
    if not _is_inside(root,svc): raise ValueError(f"serviceStart outside installRoot: {svc}")
    if not _is_inside(root,icon): raise ValueError(f"icon outside installRoot: {icon}")
    if not _is_inside(root,token): raise ValueError(f"token outside installRoot: {token}")
    try: auth_token=token.read_text(encoding="utf-8").strip()
    except FileNotFoundError: raise FileNotFoundError(f"auth token not found: {token}") from None
    if not auth_token: raise ValueError(f"auth token is empty: {token}")
    m={"schemaVersion":SCHEMA_VERSION,"productId":PRODUCT_ID,"displayName":DISPLAY_NAME,"productVersion":str(product_version),"hubCompatRange":HUB_COMPAT_RANGE,"installRoot":str(root),"serviceStart":[str(svc)],"serviceStop":["SIGTERM"],"statusEndpoint":{"host":"127.0.0.1","port":int(port),"authHeader":"Authorization","authToken":auth_token},"icon":str(icon)}
    if set(m.keys())!=ALLOWED_FIELDS: raise ValueError(f"fields mismatch: {sorted(m.keys())}")
    return m
def write_orthic_manifest(root:Path,home:Path,port:int,product_version:str,*,manifest_path:Path|None=None,win:bool|None=None)->Path:
    root=Path(root); home=Path(home)
    dest=Path(manifest_path) if manifest_path is not None else home/".orthic/hub/products.d/membrane.json"
    m=build_manifest_dict(root,port,product_version,win=win)
    if not _is_inside(root,Path(m["serviceStart"][0])): raise ValueError("serviceStart[0] escapes installRoot")
    if not _is_inside(root,Path(m["icon"])): raise ValueError("icon escapes installRoot")
    dest.parent.mkdir(parents=True,exist_ok=True)
    tmp=None
    try:
        with tempfile.NamedTemporaryFile(mode="w",encoding="utf-8",delete=False,dir=str(dest.parent),suffix=".tmp") as f:
            tmp=Path(f.name); _secure_file(tmp); json.dump(m,f,indent=2,sort_keys=True); f.write("\n")
        os.replace(tmp,dest)
    finally:
        if tmp is not None and tmp.exists():
            try: tmp.unlink()
            except OSError: pass
    return dest
