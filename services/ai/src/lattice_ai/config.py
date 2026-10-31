from pydantic import SecretStr
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    """Process configuration, read from environment variables (case-insensitive)."""

    model_config = SettingsConfigDict(extra="ignore", frozen=True)

    grpc_port: int = 50051
    http_port: int = 8000
    gateway_tools_url: str = "http://localhost:50052"
    anthropic_api_key: SecretStr | None = None
    log_level: str = "INFO"

    @property
    def gateway_tools_target(self) -> str:
        """gRPC channel target: `host:port`, without the URL scheme Compose passes in."""
        for scheme in ("http://", "https://"):
            if self.gateway_tools_url.startswith(scheme):
                return self.gateway_tools_url.removeprefix(scheme).rstrip("/")
        return self.gateway_tools_url.rstrip("/")
