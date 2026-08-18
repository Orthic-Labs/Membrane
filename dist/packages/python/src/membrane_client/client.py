"""Fail-closed client for published Membrane operation envelopes."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Mapping

Transport = Callable[[str, Mapping[str, Any]], Mapping[str, Any]]
_ENVELOPE_VERSION = 1
_ERROR_VERSION = 1
_ERROR_CODES = {
    "membrane_context": frozenset({
        "context_unavailable", "context_scope_denied", "context_workspace_no_repos",
        "context_workspace_abstained", "context_deadline_exceeded",
        "context_caller_scope_binding_denied", "context_caller_not_authorized",
        "context_cross_root_denied", "context_target_denied", "context_envelope_invalid",
    }),
    "membrane_source_read": frozenset({
        "source_read_unavailable", "source_read_hash_mismatch", "source_read_anchor_missing",
        "source_read_scope_denied", "source_read_envelope_invalid",
    }),
}


class ProtocolError(RuntimeError):
    """Typed remote failure or malformed protocol envelope."""

    def __init__(self, code: str, message: str, retryable: bool = False,
                 details: Mapping[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code, self.retryable = code, retryable
        self.details = dict(details or {})


@dataclass(frozen=True)
class ContextAnalysis:
    schema_version: int
    included: int
    omitted: int
    degraded: bool


@dataclass(frozen=True)
class ReceiptAnalysis:
    schema_version: int
    admitted_chars: int
    decision: str
    degraded: bool


def _mapping(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise ProtocolError("protocol_envelope_invalid", f"{label} must be an object")
    return value


def _closed(value: Mapping[str, Any], allowed: frozenset[str], label: str) -> None:
    if set(value) - allowed:
        raise ProtocolError("protocol_envelope_invalid", f"{label} has unknown fields")


def analyze_packet(packet: Mapping[str, Any]) -> ContextAnalysis:
    """Summarize ContextPacketV1 without reading block content."""
    packet = _mapping(packet, "packet")
    if packet.get("schemaVersion") != 1:
        raise ProtocolError("protocol_version_unsupported", "unsupported packet schemaVersion")
    blocks, omissions = packet.get("blocks"), packet.get("omissions")
    if not isinstance(blocks, list) or not isinstance(omissions, list):
        raise ProtocolError("protocol_envelope_invalid", "packet blocks and omissions must be arrays")
    return ContextAnalysis(1, len(blocks), len(omissions), bool(omissions))


def analyze_receipt(receipt: Mapping[str, Any]) -> ReceiptAnalysis:
    """Summarize current ContextReceipt v2 or prior v1 without content access."""
    receipt = _mapping(receipt, "receipt")
    version, admitted, decision = receipt.get("schemaVersion"), receipt.get("admittedChars"), receipt.get("decision")
    if version not in (1, 2):
        raise ProtocolError("protocol_version_unsupported", "unsupported receipt schemaVersion")
    if not isinstance(admitted, int) or admitted < 0 or not isinstance(decision, str) or not decision:
        raise ProtocolError("protocol_envelope_invalid", "receipt admission fields invalid")
    provider, fallback, reason = receipt.get("providerStatus"), receipt.get("fallbackMode"), receipt.get("degradationReason")
    if provider not in {"fresh", "stale", "unavailable", "degraded"} or fallback not in {"none", "non_graph_sources_only", "portable_text_only"} or not isinstance(reason, str):
        raise ProtocolError("protocol_envelope_invalid", "receipt status fields invalid")
    return ReceiptAnalysis(version, admitted, decision, provider != "fresh" or fallback != "none" or reason != "none")


class MembraneClient:
    """Calls only through supplied local transport; never opens a socket."""

    def __init__(self, transport: Transport) -> None:
        if not callable(transport):
            raise TypeError("transport must be callable")
        self._transport = transport

    def context(self, request: Mapping[str, Any]) -> Mapping[str, Any]:
        return self.call("membrane_context", request)

    def source_read(self, request: Mapping[str, Any]) -> Mapping[str, Any]:
        return self.call("membrane_source_read", request)

    def call(self, operation: str, request: Mapping[str, Any]) -> Mapping[str, Any]:
        if operation not in _ERROR_CODES:
            raise ProtocolError("operation_unsupported", f"unsupported operation: {operation}")
        response = _mapping(self._transport(operation, _mapping(request, "request")), "response")
        _closed(response, frozenset({"schemaVersion", "operation", "errorVersion", "result"}), "response")
        if response.get("schemaVersion") != _ENVELOPE_VERSION or response.get("errorVersion") != _ERROR_VERSION:
            raise ProtocolError("protocol_version_unsupported", "unsupported operation envelope version")
        if response.get("operation") != operation:
            raise ProtocolError("protocol_envelope_invalid", "response operation does not match request")
        result = _mapping(response.get("result"), "result")
        if result.get("kind") == "success":
            _closed(result, frozenset({"kind", "data"}), "success result")
            return _mapping(result.get("data"), "result.data")
        if result.get("kind") != "error":
            raise ProtocolError("protocol_envelope_invalid", "result kind must be success or error")
        _closed(result, frozenset({"kind", "code", "message", "retryable", "details"}), "error result")
        code, message = result.get("code"), result.get("message")
        if code not in _ERROR_CODES[operation] or not isinstance(message, str) or not message:
            raise ProtocolError("protocol_envelope_invalid", "unknown or malformed typed error")
        retryable = result.get("retryable")
        if not isinstance(retryable, bool):
            raise ProtocolError("protocol_envelope_invalid", "error retryable must be boolean")
        details = result.get("details", {})
        raise ProtocolError(code, message, retryable, _mapping(details, "error details"))
