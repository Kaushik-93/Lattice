# syntax=docker/dockerfile:1.7
FROM python:3.13.18-slim-trixie AS build
COPY --from=ghcr.io/astral-sh/uv:0.8.19 /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy \
    UV_PYTHON_DOWNLOADS=never
WORKDIR /app
# Dependencies first: this layer is reused until uv.lock changes.
COPY pyproject.toml uv.lock ./
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --frozen --no-dev --no-install-project
COPY src ./src
RUN --mount=type=cache,target=/root/.cache/uv \
    uv sync --frozen --no-dev --no-editable

FROM python:3.13.18-slim-trixie
RUN useradd --system --uid 10001 --no-create-home lattice
COPY --from=build --chown=lattice:lattice /app/.venv /app/.venv
ENV PATH="/app/.venv/bin:$PATH" \
    PYTHONUNBUFFERED=1
USER lattice
EXPOSE 50051 8000
CMD ["python", "-m", "lattice_ai"]
