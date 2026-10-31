from __future__ import annotations

from lattice.ai.v1 import ai_pb2, ai_pb2_grpc


async def collect(stub: ai_pb2_grpc.AiServiceAsyncStub, question: str) -> list[ai_pb2.AskEvent]:
    request = ai_pb2.AskRequest(request_id="req-1", project_id="p1", question=question)
    return [event async for event in stub.Ask(request)]


async def test_streams_progress_then_answer(ai: ai_pb2_grpc.AiServiceAsyncStub) -> None:
    events = await collect(ai, "What was net revenue by region last quarter?")

    kinds = [e.WhichOneof("event") for e in events]
    assert kinds == ["progress", "progress", "answer"]
    assert events[0].progress.stage == "received"
    assert events[1].progress.stage == "tools"
    assert "test-gateway" in events[1].progress.message
    assert "phase 4" in events[2].answer.text
    assert all(e.request_id == "req-1" for e in events)


async def test_elapsed_time_is_monotonic(ai: ai_pb2_grpc.AiServiceAsyncStub) -> None:
    events = await collect(ai, "anything")
    elapsed = [e.elapsed_us for e in events]
    assert elapsed == sorted(elapsed)


async def test_first_event_arrives_without_waiting_on_tools(
    ai: ai_pb2_grpc.AiServiceAsyncStub,
) -> None:
    events = await collect(ai, "anything")
    # The UI shows progress from 0 ms: the first event is emitted before any tool call.
    assert events[0].elapsed_us < 5_000


async def test_empty_question_fails_cleanly(ai: ai_pb2_grpc.AiServiceAsyncStub) -> None:
    events = await collect(ai, "   ")
    assert events[-1].failure.code == "invalid_request"


async def test_unreachable_gateway_tools_is_a_failure_event_not_a_crash(
    ai_without_gateway: ai_pb2_grpc.AiServiceAsyncStub,
) -> None:
    events = await collect(ai_without_gateway, "anything")
    assert events[0].progress.stage == "received"
    assert events[-1].failure.code == "gateway_tools_unavailable"
