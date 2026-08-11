"""Tests for Orthic manifest producer — pure stdlib, no launchd/IPC."""
from __future__ import annotations
import json, stat
from pathlib import Path
import orthic_manifest as om

def _setup_root(tmp_path: Path, *, with_binary=True, with_icon=True)->Path:
    root=tmp_path/"installRoot"; root.mkdir()
    (root/"tools/bin").mkdir(parents=True,exist_ok=True)
    (root/"install/assets").mkdir(parents=True,exist_ok=True)
    (root/"tools/.cache/memory").mkdir(parents=True,exist_ok=True)
    (root/"tools/.cache/memory/api-token").write_text("test-token\n",encoding="utf-8")
    (root/"engine/crates/membrane").mkdir(parents=True,exist_ok=True)
    (root/"engine/crates/membrane/Cargo.toml").write_text('[package]\nname="membrane"\nversion="0.1.0"\n',encoding="utf-8")
    if with_binary: (root/"tools/bin/crypt-service").write_text("#!bin",encoding="utf-8")
    if with_icon:
        src=Path(__file__).parent.parent/"assets/membrane-tab-icon.png"
        data=src.read_bytes() if src.is_file() else b"fake"
        (root/"install/assets/membrane-tab-icon.png").write_bytes(data)
    return root

def test_manifest_has_exact_field_set(tmp_path):
    root=_setup_root(tmp_path); home=tmp_path/"home"
    dest=om.write_orthic_manifest(root,home,47851,"0.1.0",win=False)
    data=json.loads(dest.read_text(encoding="utf-8"))
    assert stat.S_IMODE(dest.stat().st_mode)==0o600
    assert set(data.keys())==om.ALLOWED_FIELDS
    assert data["schemaVersion"]==1
    assert data["productId"]=="membrane"
    assert data["displayName"]=="Membrane"
    assert data["productVersion"]=="0.1.0"
    assert data["hubCompatRange"]==">=0.1.11 <0.2.0"
    assert data["installRoot"]==str(root)
    assert data["serviceStart"]==[str(root/"tools/bin/crypt-service")]
    assert data["serviceStop"]==["SIGTERM"]
    assert data["statusEndpoint"]["host"]=="127.0.0.1"
    assert data["statusEndpoint"]["port"]==47851
    assert data["statusEndpoint"]["authHeader"]=="Authorization"
    assert data["statusEndpoint"]["authToken"]=="test-token"
    assert "path" not in data["statusEndpoint"]
    assert "tokenPath" not in data["statusEndpoint"]
    assert data["icon"]==str(root/"install/assets/membrane-tab-icon.png")

def test_serviceStart_and_icon_inside_installRoot(tmp_path):
    root=_setup_root(tmp_path); home=tmp_path/"home"
    dest=om.write_orthic_manifest(root,home,47851,"0.1.0",win=False)
    data=json.loads(dest.read_text(encoding="utf-8"))
    svc=Path(data["serviceStart"][0]).resolve(); icn=Path(data["icon"]).resolve()
    assert svc.is_relative_to(root.resolve())
    assert icn.is_relative_to(root.resolve())
    # symlink escape: create outside target and link inside
    outside=tmp_path/"outside.bin"
    outside.write_text("x",encoding="utf-8")
    link=root/"install/assets/link.png"
    try:
        link.symlink_to(outside)
        # build should fail if icon were link escaping — we verify _is_inside rejects
        assert not om._is_inside(root,link)
    finally:
        if link.is_symlink(): link.unlink()

def test_missing_binary_refuses_and_leaves_no_partial(tmp_path):
    root=_setup_root(tmp_path,with_binary=False); home=tmp_path/"home"
    try: om.write_orthic_manifest(root,home,47851,"0.1.0",win=False)
    except FileNotFoundError: pass
    else: assert False,"expected FileNotFoundError"
    dest=home/".orthic/hub/products.d/membrane.json"
    assert not dest.exists()

def test_missing_or_empty_token_refuses_and_leaves_no_partial(tmp_path):
    root=_setup_root(tmp_path); home=tmp_path/"home"
    token=root/"tools/.cache/memory/api-token"
    token.unlink()
    try: om.write_orthic_manifest(root,home,47851,"0.1.0",win=False)
    except FileNotFoundError: pass
    else: assert False,"expected FileNotFoundError"
    token.write_text("\n",encoding="utf-8")
    try: om.write_orthic_manifest(root,home,47851,"0.1.0",win=False)
    except ValueError: pass
    else: assert False,"expected ValueError"
    assert not (home/".orthic/hub/products.d/membrane.json").exists()

def test_orthic_schema_rejects_empty_product_version_and_invalid_port(tmp_path):
    root=_setup_root(tmp_path); home=tmp_path/"home"
    for port in (0, 65536):
        try: om.write_orthic_manifest(root,home,port,"0.1.0",win=False)
        except ValueError as error: assert "statusEndpoint.port" in str(error)
        else: assert False,"expected ValueError"
    try: om.write_orthic_manifest(root,home,47851,"",win=False)
    except ValueError as error: assert "productVersion" in str(error)
    else: assert False,"expected ValueError"
    assert not (home/".orthic/hub/products.d/membrane.json").exists()

def test_atomic_write_no_tmp_leak(tmp_path):
    root=_setup_root(tmp_path); home=tmp_path/"home"
    dest=om.write_orthic_manifest(root,home,47851,"0.1.0",win=False)
    assert dest.is_file()
    # no .tmp leftovers
    assert not list(dest.parent.glob("*.tmp"))

def test_read_product_version_fallback(tmp_path):
    root=tmp_path/"empty"; root.mkdir()
    assert om.read_product_version(root)=="0.1.0"
    root2=_setup_root(tmp_path)
    assert om.read_product_version(root2)=="0.1.0"
