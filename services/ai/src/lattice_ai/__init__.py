"""Lattice AI service.

Answers natural-language questions against the governed business-logic layer. It never
connects to a data source: every read and every proposal goes through the gateway's gRPC
tool API, where RBAC, row-level security, approvals and auditing are enforced.
"""

__version__ = "0.1.0"
