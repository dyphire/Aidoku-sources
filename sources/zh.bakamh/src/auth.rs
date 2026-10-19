use crate::{BASE_URL, net};
use aidoku::{
	Result,
	alloc::String,
	helpers::uri::encode_uri_component,
	imports::{
		defaults::defaults_get,
		html::{Document, Html},
		net::Request,
	},
	prelude::*,
};

// The app stores the basic login credentials under these keys when the login succeeds.
const USERNAME_KEY: &str = "login.username";

pub fn is_logged_in() -> bool {
	defaults_get::<String>(USERNAME_KEY).is_some_and(|v| !v.is_empty())
}

/// The login nonce from `var wpMangaLogin = {…,"nonce":"…",…}`, an inline script on
/// every page for signed-out visitors.
fn login_nonce(page: &str) -> Option<String> {
	let (_, rest) = page.split_once("var wpMangaLogin")?;
	let (_, rest) = rest.split_once("\"nonce\":\"")?;
	let (nonce, _) = rest.split_once('"')?;
	(!nonce.is_empty() && nonce.chars().all(|c| c.is_ascii_alphanumeric()))
		.then(|| String::from(nonce))
}

/// The logout link carries a nonce tied to the session, so it is read off a page.
fn logout_url(doc: &Document) -> Option<String> {
	doc.select_first("a[href*=\"action=logout\"]")
		.and_then(|el| el.attr("href"))
		.filter(|href| href.starts_with("http"))
}

/// Signs in the way the site's own login form does: a successful `wp_manga_signin` sets a
/// `wordpress_logged_in_*` cookie, which the app's shared cookie jar stores and replays.
pub fn login(username: &str, password: &str) -> Result<bool> {
	let home = format!("{BASE_URL}/");
	let page = net::fetch_text(Request::get(&home)?)?;
	let nonce = match login_nonce(&page) {
		Some(nonce) => nonce,
		None => {
			// WordPress only answers wp_manga_signin for signed-out visitors; a leftover
			// session cookie hides the nonce, so end that session and read the page again.
			let doc = Html::parse_with_url(&page, &home)?;
			let Some(url) = logout_url(&doc) else {
				bail!("No login nonce found");
			};
			Request::get(url)?.send()?;
			let page = net::fetch_text(Request::get(&home)?)?;
			login_nonce(&page).ok_or_else(|| error!("No login nonce found"))?
		}
	};

	let body = format!(
		"action=wp_manga_signin&login={}&pass={}&rememberme=forever&nonce={nonce}",
		encode_uri_component(username),
		encode_uri_component(password),
	);
	let text = Request::post(format!("{BASE_URL}/wp-admin/admin-ajax.php"))?
		.header(
			"Content-Type",
			"application/x-www-form-urlencoded; charset=UTF-8",
		)
		.header("Referer", &home)
		.body(body.as_bytes())
		.string()?;

	// A wrong account or password gets 200 with {"success":false,…}; success is
	// {"success":true,"data":{"id":…}}.
	if text.contains("\"success\":true") {
		Ok(true)
	} else if text.contains("\"success\":false") {
		Ok(false)
	} else {
		bail!("Unexpected login response")
	}
}

/// Ends the site session, which clears its cookie from the shared jar.
pub fn logout() {
	let home = format!("{BASE_URL}/");
	let Some(url) = Request::get(&home)
		.ok()
		.and_then(|request| net::fetch_text(request).ok())
		.and_then(|page| Html::parse_with_url(&page, &home).ok())
		.and_then(|doc| logout_url(&doc))
	else {
		return;
	};
	if let Ok(request) = Request::get(url) {
		_ = request.send();
	}
}

