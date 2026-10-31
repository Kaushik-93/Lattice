"""The `AiService` gRPC implementation.

Phase 0 is a stub that exercises the whole boundary: it streams progress immediately,
calls back into the gateway's tool API, and finishes with a placeholder answer. The
Claude tool loop replaces the body in phase 4; the streaming contract stays.
"""

# The *AsyncStub types exist only in the generated .pyi stubs; keep annotations lazy.
from __future__ import annotations

import time
from collections.abc import AsyncIterator
from typing import override

import grpc
from prometheus_client import Counter, Histogram

from lattice.ai.v1 import ai_pb2, ai_pb2_grpc, tools_pb2, tools_pb2_grpc

TOOLS_TIMEOUT_S = 0.5

ASK_REQUESTS = Counter("lattice_ai_ask_requests_total", "Ask calls by outcome.", ["outcome"])
ASK_SECONDS = Histogram(
    "lattice_ai_ask_seconds",
    "Ask latency in seconds, excluding LLM time once phase 4 lands.",
    buckets=(0.005, 0.01, 0.025, 0.05, 0.1, 0.2, 0.5, 1.0, 3.0),
)

PLACEHOLDER_ANSWER = (
    "Lattice can't answer questions yet: the copilot arrives in phase 4. "
    "This reply confirms the gateway ⇄ AI service path works end to end."
)


class AskService(ai_pb2_grpc.AiServiceServicer):
    def __init__(self, tools: tools_pb2_grpc.GatewayToolsAsyncStub) -> None:
        self._tools = tools

    @override
    async def Ask(
        self,
        request: ai_pb2.AskRequest,
        context: grpc.aio.ServicerContext[ai_pb2.AskRequest, ai_pb2.AskEvent],
    ) -> AsyncIterator[ai_pb2.AskEvent]:
        started_ns = time.perf_counter_ns()

        def event(**fields: object) -> ai_pb2.AskEvent:
            elapsed_us = (time.perf_counter_ns() - started_ns) // 1000
            return ai_pb2.AskEvent(request_id=request.request_id, elapsed_us=elapsed_us, **fields)  # type: ignore[arg-type]

        outcome = "answered"
        try:
            yield event(progress=ai_pb2.Progress(stage="received", message="Reading your question"))

            if not request.question.strip():
                outcome = "invalid"
                yield event(
                    failure=ai_pb2.Failure(code="invalid_request", message="The question is empty.")
                )
                return

            tools_started = time.perf_counter()
            try:
                pong = await self._tools.Ping(
                    tools_pb2.PingRequest(request_id=request.request_id), timeout=TOOLS_TIMEOUT_S
                )
            except grpc.aio.AioRpcError as err:
                outcome = "tools_unavailable"
                yield event(
                    failure=ai_pb2.Failure(
                        code="gateway_tools_unavailable",
                        message=f"Gateway tools did not answer: {err.code().name}",
                    )
                )
                return
            tools_ms = (time.perf_counter() - tools_started) * 1000
            yield event(
                progress=ai_pb2.Progress(
                    stage="tools",
                    message=f"Gateway tools reachable in {tools_ms:.1f} ms "
                    f"(gateway {pong.gateway_version})",
                )
            )
            yield event(answer=ai_pb2.Answer(text=PLACEHOLDER_ANSWER))
        finally:
            ASK_REQUESTS.labels(outcome=outcome).inc()
            ASK_SECONDS.observe((time.perf_counter_ns() - started_ns) / 1e9)
