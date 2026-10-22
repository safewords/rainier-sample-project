//! The add-ins' routes.
//!
//! Each block is behind the cargo feature that brings the add-in, so a default
//! build declares none of them. Which of the *configured* services answer is
//! the environment's business — `/stripe/webhook` exists only with a webhook
//! secret to verify it against, and `/auth/gitlab/redirect` is a 404 unless
//! GitLab is declared.
//!
//! | Feature | Routes |
//! |---|---|
//! | `accounts` | `/account/*` — register, login, logout, password reset, email verification, confirm password, change password; `GET /account`; `/admin/impersonate/*` |
//! | `two-factor` | `/account/two-factor-challenge`, `/account/user/two-factor-*` |
//! | `passkeys` | `/account/passkeys/login*`, `/account/user/passkeys*` |
//! | `sso` | `/auth/{provider}/redirect`, `/auth/{provider}/callback` |
//! | `billing-stripe` | `/billing/checkout`, `/billing/portal`, `/billing/pro`, `/stripe/webhook` |
//! | `oauth-server` | `/oauth/authorize`, `/oauth/token`, `/oauth/revoke`, `/api/v1/me` |

use std::sync::Arc;

use rainier_framework::accounts::EnsureEmailIsVerified;
#[cfg(feature = "billing-stripe")]
use rainier_framework::auth::ForbidImpersonation;
use rainier_framework::prelude::*;

use crate::app::http::controllers::account_controller;
use crate::app::http::kernel;
use crate::app::models::Member;
use crate::app::providers::AddIns;

/// Declare them.
///
/// Fails when an add-in is switched on without what it needs — the
/// scaffolding checks at mount time, so a misconfiguration stops the boot
/// rather than surfacing as a 500 on the first request to reach it.
pub fn routes(router: &mut Router, addins: &Arc<AddIns>) -> Result<()> {
    // The scaffolding's own routes, under `/account` with `accounts.*` names,
    // inside the browser stack it was built with.
    addins.accounts.routes(router)?;

    router.group(GroupAttributes::new().middleware(kernel::browser(&addins.services)), |router| {
        router.get("/account/csrf", account_controller::csrf).name("accounts.csrf");

        // Registration is the scaffolding's flow on the application's route,
        // so the captcha can stand in front of it.
        #[allow(unused_variables, reason = "the captcha attaches to it")]
        let register = router
            .post("/account/register", account_controller::register)
            .name("accounts.register")
            .middleware(kernel::throttle_writes(10));
        #[cfg(feature = "captcha")]
        if let Some(captcha) = &addins.captcha {
            register.middleware(rainier_framework::RequireCaptcha::new(Arc::clone(captcha)));
        }

        #[cfg(feature = "sso")]
        {
            use crate::app::http::controllers::sso_controller;

            router.get("/auth/{provider}/redirect", sso_controller::redirect).name("sso.redirect");
            router.get("/auth/{provider}/callback", sso_controller::callback).name("sso.callback");
        }

        router.group(GroupAttributes::new().middleware(kernel::web_auth()), |router| {
            // Only a proven address sees the account page. A JSON client is
            // told 403; a browser is sent to verify.
            router
                .get("/account", account_controller::show)
                .name("account")
                .middleware(EnsureEmailIsVerified::<Member>::new(addins.guard.clone()));

            // `leave` before `{user}`: routes match in declaration order.
            router
                .post("/admin/impersonate/leave", account_controller::leave)
                .name("impersonate.leave");
            router
                .post("/admin/impersonate/{user}", account_controller::impersonate)
                .name("impersonate.take");

            #[cfg(feature = "billing-stripe")]
            {
                use crate::app::http::controllers::billing_controller;
                use crate::app::http::middleware::EnsureSubscribed;

                // Spending money is the member's to do, never support's while
                // acting as them.
                router
                    .post("/billing/checkout", billing_controller::checkout)
                    .name("billing.checkout")
                    .middleware(ForbidImpersonation);
                router
                    .post("/billing/portal", billing_controller::portal)
                    .name("billing.portal")
                    .middleware(ForbidImpersonation);
                router
                    .get("/billing/pro", billing_controller::pro)
                    .name("billing.pro")
                    .middleware(EnsureSubscribed::resolved());
            }

            #[cfg(feature = "oauth-server")]
            {
                use crate::app::http::controllers::oauth_controller;

                router.get("/oauth/authorize", oauth_controller::authorize).name("oauth.authorize");
                router.post("/oauth/authorize", oauth_controller::approve).name("oauth.approve");
            }
        });
    });

    // Stripe signs its deliveries; that signature is the whole of their
    // authentication. No session — there is no browser — and no CSRF, which
    // Stripe could not satisfy if it tried.
    #[cfg(feature = "billing-stripe")]
    if let Some(webhooks) = addins.billing.as_ref().and_then(|billing| billing.webhooks.as_ref()) {
        router.post("/stripe/webhook", webhooks.handler()).name("stripe.webhook");
    }

    #[cfg(feature = "oauth-server")]
    {
        use crate::app::http::controllers::oauth_controller;
        use rainier_framework::auth::{Authenticate, RequireAbility};

        // The client authenticates itself here, so no session and no CSRF.
        router.post("/oauth/token", oauth_controller::token).name("oauth.token");
        router.post("/oauth/revoke", oauth_controller::revoke).name("oauth.revoke");

        // The API a token unlocks, through the `oauth` guard; each route then
        // names the scope it needs.
        router.group(
            GroupAttributes::new().prefix("api/v1").name("api.v1.").middleware(
                kernel::api(None).with_stack(
                    Authenticate::<crate::app::models::User>::resolved_with_guard("oauth"),
                ),
            ),
            |router| {
                router
                    .get("/me", oauth_controller::me)
                    .name("me")
                    .middleware(RequireAbility::all(["account"]));
            },
        );
    }

    Ok(())
}
