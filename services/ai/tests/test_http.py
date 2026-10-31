import grpc
import httpx
from grpc_health.v1 import health_pb2_grpc

from lattice_ai.config import Settings
from lattice_ai.server.http import create_app


def client_for(gateway_target: str) -> tuple[httpx.AsyncClient, grpc.aio.Channel]:
    channel = grpc.aio.insecure_channel(gateway_target)
    app = create_app(health_pb2_grpc.HealthStub(channel))
    transport = httpx.ASGITransport(app=app)
    return httpx.AsyncClient(transport=transport, base_url="http://ai"), channel


async def test_liveness() -> None:
    client, channel = client_for("127.0.0.1:9")
    async with client:
        response = await client.get("/healthz")
    await channel.close()
    assert response.status_code == 200
    assert response.json() == {"status": "ok"}


async def test_ready_when_gateway_tools_serve(gateway_target: str) -> None:
    client, channel = client_for(gateway_target)
    async with client:
        response = await client.get("/readyz")
    await channel.close()
    assert response.status_code == 200
    body = response.json()
    assert body["status"] == "ready"
    assert body["checks"][0]["name"] == "gateway_tools"


async def test_not_ready_when_gateway_is_down() -> None:
    client, channel = client_for("127.0.0.1:9")
    async with client:
        response = await client.get("/readyz")
    await channel.close()
    assert response.status_code == 503
    assert response.json()["checks"][0]["ok"] is False


async def test_metrics_exposed() -> None:
    client, channel = client_for("127.0.0.1:9")
    async with client:
        response = await client.get("/metrics")
    await channel.close()
    assert response.status_code == 200
    assert "lattice_ai_ask_requests_total" in response.text


def test_gateway_url_scheme_is_stripped_for_grpc() -> None:
    assert (
        Settings(gateway_tools_url="http://gateway:50052").gateway_tools_target == "gateway:50052"
    )
    assert Settings(gateway_tools_url="gateway:50052/").gateway_tools_target == "gateway:50052"
