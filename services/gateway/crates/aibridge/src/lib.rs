//! The gateway's side of the AI boundary: a client for the Python AI service, and the
//! gRPC tool server the AI calls back into. The AI never reaches data any other way.

use std::future::Future;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tonic::transport::{Channel, Endpoint, Server};
use tonic::{Request, Response, Status};
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;

pub mod pb {
    tonic::include_proto!("lattice.ai.v1");
}

use pb::ai_service_client::AiServiceClient;
use pb::gateway_tools_server::{GatewayTools, GatewayToolsServer};
use pb::{AskEvent, AskRequest, PingRequest, PingResponse};

/// The gRPC health-check service name the AI service reports under.
pub const AI_SERVICE_NAME: &str = "lattice.ai.v1.AiService";

#[derive(Debug, thiserror::Error)]
pub enum AiBridgeError {
    #[error("invalid AI service URL: {0}")]
    InvalidUrl(#[from] tonic::transport::Error),
    #[error("AI service call failed: {0}")]
    Call(#[from] Status),
    #[error("AI service reports {0}")]
    NotServing(String),
}

/// Cheap to clone; all clones share one HTTP/2 connection.
#[derive(Clone)]
pub struct AiClient {
    ai: AiServiceClient<Channel>,
    health: HealthClient<Channel>,
}

impl AiClient {
    /// Connects on first use, so the gateway starts (and serves dashboards) even while the
    /// AI service is still booting or down.
    pub fn connect_lazy(url: &str) -> Result<Self, AiBridgeError> {
        let channel = Endpoint::from_shared(url.to_owned())?
            .connect_timeout(Duration::from_secs(1))
            .tcp_nodelay(true)
            .http2_keep_alive_interval(Duration::from_secs(30))
            .keep_alive_while_idle(true)
            .connect_lazy();
        Ok(Self {
            ai: AiServiceClient::new(channel.clone()),
            health: HealthClient::new(channel),
        })
    }

    pub async fn check_health(&self) -> Result<(), AiBridgeError> {
        let response = self
            .health
            .clone()
            .check(HealthCheckRequest {
                service: AI_SERVICE_NAME.to_owned(),
            })
            .await?
            .into_inner();
        match response.status() {
            ServingStatus::Serving => Ok(()),
            other => Err(AiBridgeError::NotServing(other.as_str_name().to_owned())),
        }
    }

    pub async fn ask(
        &self,
        request: AskRequest,
    ) -> Result<tonic::Streaming<AskEvent>, AiBridgeError> {
        Ok(self.ai.clone().ask(request).await?.into_inner())
    }
}

/// Phase 0 tool server: only `Ping`, which proves the AI → gateway path end to end.
pub struct ToolsService {
    version: &'static str,
    started: Instant,
}

impl ToolsService {
    pub fn new(version: &'static str) -> Self {
        Self {
            version,
            started: Instant::now(),
        }
    }
}

#[tonic::async_trait]
impl GatewayTools for ToolsService {
    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        tracing::debug!(request_id = %request.get_ref().request_id, "tools.ping");
        Ok(Response::new(PingResponse {
            gateway_version: self.version.to_owned(),
            uptime_s: self.started.elapsed().as_secs(),
        }))
    }
}

/// Serves the tool API plus the standard gRPC health service until `shutdown` resolves.
pub async fn serve_tools(
    addr: SocketAddr,
    version: &'static str,
    shutdown: impl Future<Output = ()>,
) -> Result<(), tonic::transport::Error> {
    let (reporter, health) = tonic_health::server::health_reporter();
    reporter
        .set_serving::<GatewayToolsServer<ToolsService>>()
        .await;
    Server::builder()
        .tcp_nodelay(true)
        .add_service(health)
        .add_service(GatewayToolsServer::new(ToolsService::new(version)))
        .serve_with_shutdown(addr, shutdown)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pb::gateway_tools_client::GatewayToolsClient;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;

    #[tokio::test]
    async fn ping_round_trip_over_real_grpc() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(
            Server::builder()
                .add_service(GatewayToolsServer::new(ToolsService::new("test")))
                .serve_with_incoming(TcpListenerStream::new(listener)),
        );

        let mut client = GatewayToolsClient::connect(format!("http://{addr}"))
            .await
            .unwrap();
        let pong = client
            .ping(PingRequest {
                request_id: "r1".into(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(pong.gateway_version, "test");
        server.abort();
    }

    #[tokio::test]
    async fn health_check_against_a_dead_ai_service_fails_fast() {
        // Port 9 (discard) is essentially never listening; connect_timeout bounds the wait.
        let client = AiClient::connect_lazy("http://127.0.0.1:9").unwrap();
        let started = Instant::now();
        assert!(client.check_health().await.is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
