"""MBR-909: prove the Python SDK package is a thin client, never the core app.

This reads the machine-checkable boundary declaration
(dist/packages/python/package-boundary.v1.json), validates its shape against the
fields schemas/sdk-python-package-boundary.v1.schema.json requires (a small
hand-rolled structural check standing in for a `jsonschema` validator: no
such package is installed in this workspace, and installing one is a new
dependency this task does not add, so this stays on the STDLIB rung), then
cross-checks the declaration against the real dist/packages/python/pyproject.toml
and the on-disk package tree. A future change that quietly starts bundling
the core app, another language's SDK, or a native binary inside
`membrane-client` fails this test instead of going unnoticed at publish time.
"""

import json
import tomllib
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).parents[2]
PY_PKG = REPO_ROOT / "dist" / "packages" / "python"
SCHEMA_PATH = REPO_ROOT / "schemas" / "sdk-python-package-boundary.v1.schema.json"
BOUNDARY_PATH = PY_PKG / "package-boundary.v1.json"


def _load_json(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


class PackageBoundarySchemaShapeTest(unittest.TestCase):
    """Structural check standing in for a jsonschema validator."""

    def setUp(self):
        self.schema = _load_json(SCHEMA_PATH)
        self.boundary = _load_json(BOUNDARY_PATH)

    def test_schema_declares_the_fields_this_suite_relies_on(self):
        self.assertEqual(
            set(self.schema["required"]),
            {
                "schemaVersion", "distributionName", "packageRoot",
                "allowedTopLevelPackages", "forbiddenPathPrefixes",
                "forbiddenFileExtensions", "runtimeDependencies", "corePublishing",
            },
        )
        self.assertEqual(self.schema["properties"]["schemaVersion"], {"const": 1})
        self.assertFalse(self.schema["additionalProperties"])

    def test_boundary_document_satisfies_every_required_field_and_type(self):
        for key in self.schema["required"]:
            self.assertIn(key, self.boundary, f"boundary document missing {key!r}")
        self.assertEqual(self.boundary["schemaVersion"], 1)
        self.assertIsInstance(self.boundary["distributionName"], str)
        self.assertTrue(self.boundary["distributionName"])
        self.assertIsInstance(self.boundary["allowedTopLevelPackages"], list)
        self.assertTrue(self.boundary["allowedTopLevelPackages"])
        self.assertIsInstance(self.boundary["forbiddenPathPrefixes"], list)
        self.assertTrue(self.boundary["forbiddenPathPrefixes"])
        self.assertIsInstance(self.boundary["forbiddenFileExtensions"], list)
        self.assertTrue(self.boundary["forbiddenFileExtensions"])
        self.assertEqual(self.boundary["runtimeDependencies"], [])
        core = self.boundary["corePublishing"]
        self.assertEqual(set(core), {"publishesCoreApp", "bundlesDaemonBinary", "note"})
        self.assertIs(core["publishesCoreApp"], False)
        self.assertIs(core["bundlesDaemonBinary"], False)
        self.assertTrue(core["note"])

    def test_no_unknown_top_level_fields_in_the_boundary_document(self):
        self.assertTrue(set(self.boundary) <= set(self.schema["properties"]))


class PackageBoundaryMatchesRealPackageTest(unittest.TestCase):
    """Cross-checks the declaration against the actual pyproject.toml + tree."""

    def setUp(self):
        self.boundary = _load_json(BOUNDARY_PATH)
        with open(PY_PKG / "pyproject.toml", "rb") as handle:
            self.pyproject = tomllib.load(handle)

    def test_distribution_name_matches_pyproject(self):
        self.assertEqual(self.boundary["distributionName"], self.pyproject["project"]["name"])

    def test_runtime_dependencies_are_empty_in_both_places(self):
        self.assertEqual(self.boundary["runtimeDependencies"], [])
        self.assertEqual(self.pyproject["project"].get("dependencies", []), [])

    def test_package_root_matches_setuptools_find_where(self):
        where = self.pyproject["tool"]["setuptools"]["packages"]["find"]["where"]
        self.assertEqual(where, [self.boundary["packageRoot"]])

    def test_discovered_packages_equal_the_allowlist(self):
        """Reimplements setuptools' `find:` rule (a directory containing
        __init__.py is a package) directly against the filesystem: setuptools
        itself is not installed in this environment, and this task does not
        install it (no new dependency; see MBR-909 evidence/commands.log)."""
        root = PY_PKG / self.boundary["packageRoot"]
        discovered = sorted(
            path.parent.name
            for path in root.rglob("__init__.py")
            if "__pycache__" not in path.parts and path.parent.parent == root
        )
        self.assertEqual(discovered, sorted(self.boundary["allowedTopLevelPackages"]))

    def test_pyproject_text_never_references_a_forbidden_path_prefix(self):
        text = (PY_PKG / "pyproject.toml").read_text(encoding="utf-8")
        for prefix in self.boundary["forbiddenPathPrefixes"]:
            self.assertNotIn(prefix, text, f"pyproject.toml references forbidden path {prefix!r}")

    def test_package_tree_contains_no_forbidden_file_extension(self):
        forbidden = tuple(self.boundary["forbiddenFileExtensions"])
        offenders = [
            str(path.relative_to(PY_PKG))
            for path in PY_PKG.rglob("*")
            if path.is_file() and "__pycache__" not in path.parts and path.name.endswith(forbidden)
        ]
        self.assertEqual(offenders, [])

    def test_py_typed_marker_is_shipped_and_declared(self):
        marker = PY_PKG / self.boundary["packageRoot"] / "membrane_client" / "py.typed"
        self.assertTrue(marker.is_file(), "py.typed marker is missing")
        package_data = self.pyproject["tool"]["setuptools"]["package-data"]
        self.assertIn("py.typed", package_data.get("membrane_client", []))

    def test_include_package_data_is_explicitly_disabled(self):
        """Guards against a future edit silently pulling stray files (docs,
        evidence, fixtures) into the sdist via setuptools' default
        include-package-data auto-discovery."""
        self.assertIs(self.pyproject["tool"]["setuptools"]["include-package-data"], False)

    def test_readme_and_license_stay_inside_the_package_directory(self):
        """A readme/license path that climbs out of dist/packages/python (e.g.
        '../../docs/...') is unverifiable by this task, since no build
        backend is installed to prove it resolves; keeping both references
        inside the package directory keeps the declared metadata testable."""
        readme = self.pyproject["project"]["readme"]
        self.assertNotIn("..", readme)
        self.assertTrue((PY_PKG / readme).is_file())
        license_text = self.pyproject["project"]["license"]["text"]
        self.assertIn("Orthic Labs Source Use License", license_text)


if __name__ == "__main__":
    unittest.main()
