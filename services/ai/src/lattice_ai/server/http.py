"""Small HTTP side-car for probes and metrics. Questions never arrive over HTTP."""

import time
from typing import Literal

from fastapi import FastAPI, Response
from grpc_health.v1 import health_pb2, health_pb2_grpc
from prometheus_client import CONTENT_TYPE_LATEST, generate_latest
from pydantic import BaseModel

from lattice_ai import __version__

PROBE_TIMEOUT_S = 1.0


class DependencyCheck(BaseModel):
    name: str
    ok: bool
    latency_ms: float
    detail: str | None = None


class ReadyReport(BaseModel):
    status: Literal["ready", "unavailable"]
    version: str
    checks: list[DependencyCheck]


def create_app(gateway_health: health_pb2_grpc.HealthStub) -> FastAPI:
    app = FastAPI(title="lattice-ai", docs_url=None, redoc_url=None, openapi_url=None)

    @app.get("/healthz")
    async def healthz() -> dict[str, str]:
        return {"status": "ok"}

    @app.get("/readyz")
    async def readyz(response: Response) -> ReadyReport:
        check = await _probe_gateway(gateway_health)
        if not check.ok:
            response.status_code = 503
        return ReadyReport(
            status="ready" if check.ok else "unavailable", version=__version__, checks=[check]
        )

    @app.get("/metrics")
    async def metrics() -> Response:
        return Response(generate_latest(), media_type=CONTENT_TYPE_LATEST)

    return app


async def _probe_gateway(stub: health_pb2_grpc.HealthStub) -> DependencyCheck:
    started = time.perf_counter()
    detail: str | None = None
    try:
        reply = await stub.Check(health_pb2.HealthCheckRequest(), timeout=PROBE_TIMEOUT_S)
        ok = reply.status == health_pb2.HealthCheckResponse.SERVING
        if not ok:
            detail = health_pb2.HealthCheckResponse.ServingStatus.Name(reply.status)
    except Exception as err:  # any failure to reach the gateway means "not ready"
        ok, detail = False, f"{type(err).__name__}: {err}"
    latency_ms = round((time.perf_counter() - started) * 1000, 2)
    return DependencyCheck(name="gateway_tools", ok=ok, latency_ms=latency_ms, detail=detail)
