"""Canonical scoped identity for Morph preference records.

Rule identity is always ``(scope, record_id)``, never a bare synthesis name
and never whatever shape a caller happened to use as a dict key. The same
record id may legitimately exist under two scope spellings across machines;
duplicate detection and update resolution must use the scoped key.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable


@dataclass(frozen=True)
class RuleKey:
  scope: str
  record_id: str

  def formatted(self) -> str:
    if self.scope:
      return f"{self.scope}/{self.record_id}"
    return self.record_id

  @classmethod
  def from_row(cls, key: str, row: dict[str, Any]) -> RuleKey:
    scope = str(row.get("scope") or "").strip()
    record_id = str(
      row.get("id") or row.get("name") or _bare_id_from_key(key)
    ).strip()
    if "/" in key and not scope:
      parsed = cls.parse(key)
      scope = parsed.scope
      if not record_id:
        record_id = parsed.record_id
    return cls(scope=scope, record_id=record_id)

  @classmethod
  def parse(cls, value: str) -> RuleKey:
    text = (value or "").strip()
    if "/" in text:
      scope, record_id = text.split("/", 1)
      return cls(scope=scope.strip(), record_id=record_id.strip())
    return cls(scope="", record_id=text)

  @classmethod
  def for_target(cls, *, name: str, scope: str | None) -> RuleKey:
    parsed = cls.parse(name)
    resolved_scope = (scope or parsed.scope or "").strip()
    record_id = parsed.record_id or name.strip()
    return cls(scope=resolved_scope, record_id=record_id)


def _bare_id_from_key(key: str) -> str:
  if "/" in key:
    return key.split("/", 1)[1]
  return key


class RuleIndex:
  """Indexes canonical rules by scoped key and bare record id."""

  def __init__(
    self,
    by_key: dict[RuleKey, dict[str, Any]],
    by_id: dict[str, list[RuleKey]],
  ) -> None:
    self._by_key = by_key
    self._by_id = by_id

  @classmethod
  def from_mapping(cls, rules: dict[str, dict[str, Any]]) -> RuleIndex:
    by_key: dict[RuleKey, dict[str, Any]] = {}
    by_id: dict[str, list[RuleKey]] = {}
    for key, row in (rules or {}).items():
      if not isinstance(row, dict):
        continue
      rk = RuleKey.from_row(str(key), row)
      if rk in by_key:
        raise ValueError(f"duplicate canonical Morph identity: {rk.formatted()}")
      by_key[rk] = row
      by_id.setdefault(rk.record_id, []).append(rk)
    return cls(by_key=by_key, by_id=by_id)

  @property
  def by_key(self) -> dict[RuleKey, dict[str, Any]]:
    return self._by_key

  def keys(self) -> set[RuleKey]:
    return set(self._by_key)

  def formatted_keys(self) -> set[str]:
    return {key.formatted() for key in self._by_key}

  def resolve(
    self,
    name: str,
    *,
    scope: str | None = None,
    required: bool = False,
  ) -> tuple[RuleKey | None, dict[str, Any] | None]:
    target = RuleKey.for_target(name=name, scope=scope)
    if target in self._by_key:
      return target, self._by_key[target]
    matches = [
      key for key in self._by_id.get(target.record_id, ())
      if not target.scope or key.scope == target.scope
    ]
    if len(matches) == 1:
      key = matches[0]
      return key, self._by_key[key]
    if len(matches) > 1:
      return None, None
    if required:
      return None, None
    return None, None

  def has(self, key: RuleKey) -> bool:
    return key in self._by_key

  def keys_for_id(self, record_id: str) -> list[RuleKey]:
    return list(self._by_id.get(record_id, ()))

  def values(self) -> Iterable[dict[str, Any]]:
    return self._by_key.values()
