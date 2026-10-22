//! The outbound HTTP transport the add-ins call third parties through.
//!
//! One transport for Stripe, GitHub, the captcha provider and Discord, chosen
//! by [`Mode`]: the real one when running, and under test the framework's
//! **fake**, bound into the container so a feature test can queue the answers
//! a provider would give and read back exactly what was sent.
//!
//! The captcha gets a fake of its own under test. Answers queued on a fake are
//! consumed in order, so one shared between a registration's captcha check and
//! a checkout's Stripe call would hand each the other's answer.

use std::sync::Arc;

use rainier_framework::http_client::{FakeTransport, Transport};

use crate::bootstrap::Mode;

/// The transport, and the fake behind it when there is one.
#[derive(Clone)]
pub struct Outbound {
    /// What the add-ins send through.
    pub http: Arc<dyn Transport>,
    /// The same transport, typed, under test — `None` when running.
    pub fake: Option<Arc<FakeTransport>>,
}

/// The captcha's own fake, under test. See the module docs.
#[derive(Clone)]
pub struct CaptchaFake(pub Arc<FakeTransport>);

impl Outbound {
    /// The transport for `mode`.
    pub fn for_mode(mode: Mode) -> Self {
        match mode {
            Mode::Running => Self {
                http: Arc::new(rainier_framework::http_client::ReqwestTransport::new()),
                fake: None,
            },
            Mode::Testing => {
                let fake = Arc::new(FakeTransport::new());
                Self { http: Arc::clone(&fake) as Arc<dyn Transport>, fake: Some(fake) }
            }
        }
    }
}

impl std::fmt::Debug for Outbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outbound").field("transport", &self.http.name()).finish()
    }
}
