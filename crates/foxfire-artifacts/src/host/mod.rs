mod artifacts;
mod error;
mod file;
mod mcp;
mod session;

use crate::store::ArtifactStore;
use agent_client_protocol::v2::ConnectionTo;
use agent_client_protocol::v2::schema::{
    InitializeProxyRequest, InitializeProxyResponse, NewSessionRequest, NewSessionResponse, ResumeSessionRequest,
    ResumeSessionResponse,
};
use agent_client_protocol::{
    Agent, Channel, Client, Conductor, ConnectTo, Error, Proxy, Responder, V2, on_receive_request,
};
use agent_client_protocol_conductor::{ConductorImpl, ProxiesAndAgent};
use agent_client_protocol_polyfill::mcp_over_acp::McpOverAcpPolyfill;
use artifacts::{Artifacts, ArtifactsHandle};
use foxfire_acp::artifact::{ArtifactFetchParams, ArtifactFetchResponse};
use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use mcp::tool;
use std::future::pending;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

pub struct ArtifactHost<T, U> {
    agent: T,
    store: U,
}

#[derive(Clone)]
struct ArtifactProxy {
    supported: Arc<AtomicBool>,
    artifacts: ArtifactsHandle,
}

impl<T: ConnectTo<Client>, U: ArtifactStore> ArtifactHost<T, U> {
    pub fn new(agent: T, store: U) -> Self {
        Self { agent, store }
    }
}

impl<T: ConnectTo<Client>, U: ArtifactStore> ConnectTo<Client> for ArtifactHost<T, U> {
    async fn connect_to(self, client: impl ConnectTo<Agent>) -> Result<(), Error> {
        let (artifacts, inbox) = ArtifactsHandle::new();
        let proxy = ArtifactProxy { supported: Arc::default(), artifacts };
        let (initialize, open, resume, fetch) = (proxy.clone(), proxy.clone(), proxy.clone(), proxy);
        let proxy = Proxy
            .v2()
            .name("foxfire-artifacts")
            .on_receive_request_from(
                Client,
                async move |req, responder, cx| initialize.initialize(req, responder, &cx),
                on_receive_request!(),
            )
            .on_receive_request_from(
                Client,
                async move |req, responder, cx| open.new_session(req, responder, &cx),
                on_receive_request!(),
            )
            .on_receive_request_from(
                Client,
                async move |req, responder, cx| resume.resume_session(req, responder, &cx),
                on_receive_request!(),
            )
            .on_receive_request_from(
                Client,
                async move |req, responder, cx| fetch.fetch(req, responder, &cx),
                on_receive_request!(),
            )
            .with_spawned(move |cx| Artifacts::new(cx, self.store).run(inbox));
        let (agent, driver) = self.agent.into_channel_and_future();
        let (tx, rx) = unbounded();
        let chain = ProxiesAndAgent::new(Channel::new(rx, agent.tx)).proxy(proxy).proxy(McpOverAcpPolyfill::http());
        tokio::select! {
            result = ConductorImpl::new_agent("foxfire", chain).connect_to(client) => result,
            result = agent.rx.map(Ok).forward(tx) => result.map_err(Error::into_internal_error),
            result = async { if let Some(driver) = driver { driver.await?; } pending().await } => result,
        }
    }
}

impl ArtifactProxy {
    fn initialize(
        &self,
        req: InitializeProxyRequest,
        responder: Responder<InitializeProxyResponse, V2>,
        cx: &ConnectionTo<Conductor>,
    ) -> Result<(), Error> {
        let supported = self.supported.clone();
        cx.send_request_to(Agent, req.initialize)
            .map(move |response| {
                let reachable = response
                    .capabilities
                    .session
                    .as_ref()
                    .and_then(|s| s.mcp.as_ref())
                    .is_some_and(|m| m.acp.is_some());
                supported.store(reachable, Ordering::Release);
                if !reachable {
                    tracing::warn!("agent does not support artifact tools");
                }
                Ok(response.into())
            })
            .forward_response_to(responder)
    }

    fn new_session(
        &self,
        req: NewSessionRequest,
        responder: Responder<NewSessionResponse, V2>,
        cx: &ConnectionTo<Conductor>,
    ) -> Result<(), Error> {
        if !self.supported.load(Ordering::Acquire) {
            return cx.send_request_to(Agent, req).forward_response_to(responder);
        }
        let session = Arc::new(OnceLock::new());
        cx.build_session_from(req)
            .with_mcp_server(tool(session.clone(), self.artifacts.clone()))?
            .on_proxy_session_start(responder, async move |opened| {
                let _ = session.set(opened.response.session_id);
                Ok(())
            })
    }

    fn resume_session(
        &self,
        req: ResumeSessionRequest,
        responder: Responder<ResumeSessionResponse, V2>,
        cx: &ConnectionTo<Conductor>,
    ) -> Result<(), Error> {
        if !self.supported.load(Ordering::Acquire) {
            return cx.send_request_to(Agent, req).forward_response_to(responder);
        }
        let (id, replay, artifacts) = (req.session_id.clone(), req.replay_from.is_some(), self.artifacts.clone());
        cx.resume_session_from(req)
            .with_mcp_server(tool(Arc::new(OnceLock::from(id.clone())), artifacts.clone()))?
            .on_proxy_session_start(responder, async move |_| if replay { artifacts.replay(id) } else { Ok(()) })
    }

    fn fetch(
        &self,
        req: ArtifactFetchParams,
        responder: Responder<ArtifactFetchResponse, V2>,
        cx: &ConnectionTo<Conductor>,
    ) -> Result<(), Error> {
        let artifacts = self.artifacts.clone();
        cx.spawn(async move {
            let fetched = artifacts.fetch(req.session_id, req.id).await;
            responder.respond_with_result(fetched.map(|content| ArtifactFetchResponse { content }))
        })
    }
}
