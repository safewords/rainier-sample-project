//! `HomeController` — an HTML page, rendered through a view.

use rainier_framework::prelude::*;
use rainier_framework::view::View as ViewTemplate;

use crate::app::http::controllers::post_controller::resolve;
use crate::app::repositories::PostRepository;

/// `GET /` — the front page.
pub async fn index() -> Result<Response> {
    let posts = resolve::<PostRepository>()?;
    let recent = posts.published_page(1, 5, None).await?;

    let name: String = Config::instance().get_or("app.name", "Rainier".into());

    let view = ViewTemplate::new("home").add("title", &name)?.add("posts", &recent.data)?;

    // `Html` rather than `Response::html`, so the content type is set for us.
    Ok(Html(View::instance().render_view(&view)?).into_response())
}

/// `GET /health` — a liveness probe. Deliberately does no I/O: it answers
/// "is this process up", not "is every dependency healthy", and conflating the
/// two makes a database blip look like a dead app to an orchestrator.
pub async fn health() -> Response {
    Response::json(&serde_json::json!({ "status": "ok" }))
}

/// `GET /visits` — the session, and flash data, in eight lines.
///
/// Behind the `web` group, which includes `session`. On a route outside it
/// `request.session()` is `None`, and this would answer `0` forever.
pub async fn visits(request: Req) -> Result<Response> {
    let session = request
        .session()
        .ok_or_else(|| Error::internal("this route needs the `session` middleware"))?;

    let visits: u64 = session.get("visits").unwrap_or(0);
    session.put("visits", visits + 1)?;

    // Flash data survives exactly one further request, so the *next* response
    // shows this and the one after does not — the redirect-then-show-a-message
    // pattern, with nothing to clean up.
    let previous = session.string("greeting");
    session.flash("greeting", format!("Seen you {} time(s) before.", visits))?;

    Ok(Response::json(&serde_json::json!({
        "visits": visits,
        "flashed_last_time": previous,
        "csrf_token": session.token(),
    })))
}
