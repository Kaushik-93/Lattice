"""Process wiring: the gRPC server (port 50051) and the HTTP probe server (port 8000)
share one asyncio loop and shut down together."""

import logging

import grpc
import uvicorn
from grpc_health.v1 import health, health_pb2, health_pb2_grpc

from lattice.ai.v1 import ai_pb2_grpc, tools_pb2_grpc
from lattice_ai.config import Settings
from lattice_ai.server.ask import AskService
from lattice_ai.server.http import create_app

log = logging.getLogger("lattice_ai")

AI_SERVICE_NAME = "lattice.ai.v1.AiService"


async def build_grpc_server(
    tools_channel: grpc.aio.Channel, bind: str
) -> tuple[grpc.aio.Server, int]:
    """Returns the started server and the port it bound (useful when `bind` ends in :0)."""
    server = grpc.aio.server()
    ai_pb2_grpc.add_AiServiceServicer_to_server(
        AskService(tools_pb2_grpc.GatewayToolsStub(tools_channel)), server
    )
    health_servicer = health.aio.HealthServicer()
    health_pb2_grpc.add_HealthServicer_to_server(health_servicer, server)
    for name in (AI_SERVICE_NAME, ""):
        await health_servicer.set(name, health_pb2.HealthCheckResponse.SERVING)
    port = server.add_insecure_port(bind)
    await server.start()
    return server, port


async def serve(settings: Settings) -> None:
    tools_channel = grpc.aio.insecure_channel(settings.gateway_tools_target)
    grpc_server, port = await build_grpc_server(tools_channel, f"[::]:{settings.grpc_port}")
    log.info(
        "lattice-ai gRPC on :%d, HTTP on :%d, gateway tools at %s",
        port,
        settings.http_port,
        settings.gateway_tools_target,
    )

    http = uvicorn.Server(
        uvicorn.Config(
            create_app(health_pb2_grpc.HealthStub(tools_channel)),
            host="0.0.0.0",
            port=settings.http_port,
            log_level=settings.log_level.lower(),
            access_log=False,
        )
    )
    try:
        # uvicorn owns SIGINT/SIGTERM handling and returns once asked to stop.
        await http.serve()
    finally:
        await grpc_server.stop(grace=5)
        await tools_channel.close()
        log.info("lattice-ai stopped")
