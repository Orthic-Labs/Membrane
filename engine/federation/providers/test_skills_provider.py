from __future__ import annotations

from pathlib import Path

from federation.providers import skills


def test_skills_provider_ranks_only_the_engine_snapshot_and_returns_its_generation(
    monkeypatch, tmp_path: Path
):
    filesystem_skill = tmp_path / "tools" / "skills" / "demo" / "SKILL.md"
    filesystem_skill.parent.mkdir(parents=True)
    filesystem_skill.write_text("filesystem generation G2\n", encoding="utf-8")
    engine_generation = "sha256:" + "1" * 64
    monkeypatch.setattr(
        skills,
        "_snapshot_from_serve",
        lambda: {
            "schemaVersion": 1,
            "generation": engine_generation,
            "skills": [
                {
                    "name": "demo",
                    "description": "demo engine snapshot",
                    "bodyHash": "2" * 64,
                }
            ],
        },
        raising=False,
    )
    monkeypatch.setattr(skills, "_WORKSPACE_ROOT", tmp_path)

    candidates, generation, warnings = skills.produce(tmp_path, "use demo", None)

    assert generation == engine_generation
    assert warnings == []
    assert [candidate["id"] for candidate in candidates] == ["skills:demo"]
    assert candidates[0]["bodyHash"] == "2" * 64
    assert "filesystem generation G2" not in str(candidates)
