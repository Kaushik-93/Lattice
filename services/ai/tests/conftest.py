# The *AsyncStub types exist only in the generated .pyi stubs; keep annotations lazy.
from __future__ import annotations

from collections.abc import AsyncIterator
from typing import override

import grpc
import pytest
from grpc_health.v1 import health, health_pb2, health_pb2_grpc

from lattice.ai.v1 import ai_pb2_grpc, tools_pb2, tools_pb2_grpc
from lattice_ai.server.app import build_grpc_server


class FakeGatewayTools(tools_pb2_grpc.GatewayToolsServicer):
    @override
    async def Ping(
        self,
        request: tools_pb2.PingRequest,
        context: grpc.aio.ServicerContext[tools_pb2.PingRequest, tools_pb2.PingResponse],
    ) -> tools_pb2.PingResponse:
        return tools_pb2.PingResponse(gateway_version="test-gateway", uptime_s=1)


@pytest.fixture
async def gateway_target() -> AsyncIterator[str]:
    """A stand-in for the Rust gateway's tool server, with the standard health service."""
    server = grpc.aio.server()
    tools_pb2_grpc.add_GatewayToolsServicer_to_server(FakeGatewayTools(), server)
    health_servicer = health.aio.HealthServicer()
    health_pb2_grpc.add_HealthServicer_to_server(health_servicer, server)
    await health_servicer.set("", health_pb2.HealthCheckResponse.SERVING)
    port = server.add_insecure_port("127.0.0.1:0")
    await server.start()
    yield f"127.0.0.1:{port}"
    await server.stop(grace=None)


async def _ai_stub(tools_target: str) -> AsyncIterator[ai_pb2_grpc.AiServiceAsyncStub]:
    tools_channel = grpc.aio.insecure_channel(tools_target)
    server, port = await build_grpc_server(tools_channel, "127.0.0.1:0")
    client_channel = grpc.aio.insecure_channel(f"127.0.0.1:{port}")
    yield ai_pb2_grpc.AiServiceStub(client_channel)
    await client_channel.close()
    await server.stop(grace=None)
    await tools_channel.close()


@pytest.fixture
async def ai(gateway_target: str) -> AsyncIterator[ai_pb2_grpc.AiServiceAsyncStub]:
    async for stub in _ai_stub(gateway_target):
        yield stub


@pytest.fixture
async def ai_without_gateway() -> AsyncIterator[ai_pb2_grpc.AiServiceAsyncStub]:
    # Port 9 (discard) is essentially never listening.
    async for stub in _ai_stub("127.0.0.1:9"):
        yield stub
