#![no_std]

use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, DynamicListings, FilterValue, ImageRequestProvider,
	Listing, ListingProvider, Manga, MangaPageResult, Page, PageContent, PageContext, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::{QueryParameters, encode_uri_component},
	imports::{
		error::AidokuError,
		html::Document,
		net::{Request, Response},
		std::parse_date,
	},
	prelude::*,
};

mod home;
mod models;
mod parser;
mod settings;

use parser::*;
use settings::*;
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

struct EHentai;

fn eh_get_html(url: &str, cookies: &str, user_agent: &str) -> Result<Document> {
	let do_request = |cookie_header: &str| -> Result<Response> {
		Ok(Request::get(url)?
			.header("Cookie", cookie_header)
			.header("User-Agent", user_agent)
			.send()?)
	};

	let mut resp = do_request(cookies)?;

	if url.contains("exhentai.org")
		&& let Some(set_cookie) = resp.get_header("Set-Cookie")
	{
		refresh_igneous_from_set_cookie(&set_cookie);
	}

	let doc = resp.get_html()?;

	if url.contains("exhentai.org") {
		let is_rejected = doc
			.select_first("body")
			.and_then(|b| b.select_first("div"))
			.is_none();

		if is_rejected {
			let base_cookie = {
				let member_id = get_ipb_member_id();
				let pass_hash = get_ipb_pass_hash();
				let mut parts: Vec<String> = Vec::new();
				parts.push("nw=1".into());
				if !member_id.is_empty() {
					parts.push(format!("ipb_member_id={}", member_id));
				}
				if !pass_hash.is_empty() {
					parts.push(format!("ipb_pass_hash={}", pass_hash));
				}
				parts.join("; ")
			};

			if let Ok(probe_req) = Request::get("https://exhentai.org")
				&& let Ok(probe) = probe_req
					.header("Cookie", &base_cookie)
					.header("User-Agent", user_agent)
					.send() && let Some(set_cookie) = probe.get_header("Set-Cookie")
			{
				refresh_igneous_from_set_cookie(&set_cookie);
			}

			let refreshed_cookies = build_cookie_header();
			resp = do_request(&refreshed_cookies)?;

			if let Some(set_cookie) = resp.get_header("Set-Cookie") {
				refresh_igneous_from_set_cookie(&set_cookie);
			}

			let retry_doc = resp.get_html()?;
			let still_rejected = retry_doc
				.select_first("body")
				.and_then(|b| b.select_first("div"))
				.is_none();
			if still_rejected {
				return Err(AidokuError::Message(
					"Access denied by ExHentai. Please check your account permissions or re-login."
						.into(),
				));
			}
			return Ok(retry_doc);
		}
	}

	Ok(doc)
}

fn rewrite_domain(url: &str) -> String {
	let domain = get_domain();
	if url.contains("exhentai.org") {
		url.replacen("exhentai.org", &domain, 1)
	} else if url.contains("e-hentai.org") {
		url.replacen("e-hentai.org", &domain, 1)
	} else {
		url.to_string()
	}
}

fn get_api_url() -> &'static str {
	"https://api.e-hentai.org/api.php"
}

fn api_showpage(
	gid: &str,
	imgkey: &str,
	page: u32,
	showkey: &str,
	nl: Option<&str>,
	cookies: &str,
) -> Option<(String, Option<String>)> {
	let nl_val = nl.unwrap_or("");
	let body = format!(
		r#"{{"method":"showpage","gid":{gid},"imgkey":"{imgkey}","page":{page},"showkey":"{showkey}","nl":"{nl_val}"}}"#
	);
	let mut resp = Request::post(get_api_url())
		.ok()?
		.header("Content-Type", "application/json")
		.header("Cookie", cookies)
		.header("User-Agent", USER_AGENT)
		.header("Referer", &get_base_url())
		.body(body.as_bytes())
		.send()
		.ok()?;

	let json: serde_json::Value = resp.get_json().ok()?;
	let i3 = json.get("i3").and_then(|v| v.as_str())?;
	let img_url = extract_src_from_img_html(i3)?;
	let nl_out = json
		.get("i6")
		.and_then(|v| v.as_str())
		.and_then(extract_nl_from_i6);

	Some((img_url.to_string(), nl_out))
}

fn extract_src_from_img_html(html: &str) -> Option<&str> {
	let start = html
		.find("src=\"")
		.map(|i| (i + 5, '"'))
		.or_else(|| html.find("src='").map(|i| (i + 5, '\'')))?;
	let (idx, quote) = start;
	let end = html[idx..].find(quote)?;
	Some(&html[idx..idx + end])
}

fn extract_nl_from_i6(i6: &str) -> Option<String> {
	let start = i6.find("nl('")?;
	let after = &i6[start + 4..];
	let end = after.find('\'')?;
	Some(after[..end].to_string())
}

fn api_imagedispatch(
	gid: &str,
	imgkey: &str,
	page: u32,
	mpvkey: &str,
	nl: Option<&str>,
	cookies: &str,
) -> Option<(String, Option<String>)> {
	let nl_val = nl.unwrap_or("");
	let body = format!(
		r#"{{"method":"imagedispatch","gid":{gid},"imgkey":"{imgkey}","page":{page},"mpvkey":"{mpvkey}","nl":"{nl_val}"}}"#
	);
	let mut resp = Request::post(get_api_url())
		.ok()?
		.header("Content-Type", "application/json")
		.header("Cookie", cookies)
		.header("User-Agent", USER_AGENT)
		.header("Referer", &get_base_url())
		.body(body.as_bytes())
		.send()
		.ok()?;

	let json: serde_json::Value = resp.get_json().ok()?;
	let img_url = json.get("i").and_then(|v| v.as_str())?.to_string();
	let nl_out = json
		.get("s")
		.and_then(|v| v.as_str())
		.filter(|s| !s.is_empty())
		.map(|s| s.to_string());

	Some((img_url, nl_out))
}

impl Source for EHentai {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		// Quick open: if query is a gallery URL, "<gid> <token>", or "<gid>/<token>",
		// return that gallery directly.
		if let Some(q) = &query {
			let q_trim = q.trim();
			if q_trim.starts_with("http://") || q_trim.starts_with("https://") {
				if q_trim.contains("e-hentai.org/g/") || q_trim.contains("exhentai.org/g/") {
					// Rewrite domain to match current setting, then normalize
					let rewritten = rewrite_domain(q_trim);
					let normalized = normalize_gallery_url(&rewritten);
					let html = eh_get_html(&normalized, &build_cookie_header(), USER_AGENT)?;
					if let Some(gallery) = parse_gallery_detail(&html, &normalized) {
						let manga: Manga = gallery.into();
						return Ok(MangaPageResult {
							entries: vec![manga],
							has_next_page: false,
						});
					}
				}
			} else if let Some((gid, token)) = parse_gid_token(q_trim) {
				// Use current base_url so exhentai users get exhentai links
				let url = format!("{}/g/{gid}/{token}/", get_base_url());
				let html = eh_get_html(&url, &build_cookie_header(), USER_AGENT)?;
				if let Some(gallery) = parse_gallery_detail(&html, &url) {
					let manga: Manga = gallery.into();
					return Ok(MangaPageResult {
						entries: vec![manga],
						has_next_page: false,
					});
				}
			}
		}
		let base_url = get_base_url();
		let cookies = build_cookie_header();

		let mut qs = QueryParameters::new();
		qs.push("f_apply", Some("Apply Filter"));

		let mut query_str = query.unwrap_or_default();
		let mut sort_index: i32 = 0;

		let mut cat_mask: u32 = 0;
		let cat_flags: &[(&str, u32)] = &[
			("f_doujinshi", 2),
			("f_manga", 4),
			("f_artistcg", 8),
			("f_gamecg", 16),
			("f_western", 512),
			("f_non-h", 256),
			("f_imageset", 32),
			("f_cosplay", 64),
			("f_asianporn", 128),
			("f_misc", 1),
		];
		const ALL_CATS: u32 = 2 + 4 + 8 + 16 + 512 + 256 + 32 + 64 + 128 + 1; // 1023
		let mut cats_filtered = false;

		let mut min_pages: Option<String> = None;
		let mut max_pages: Option<String> = None;
		let mut min_rating: i32 = 0;
		let mut tag_filter: Option<String> = None;
		let mut disable_custom: Vec<String> = Vec::new();

		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => {
					sort_index = index;
				}
				FilterValue::MultiSelect { id, included, .. } => {
					if id == "categories" {
						cats_filtered = true;
						for flag_id in &included {
							if let Some(&(_, mask)) = cat_flags.iter().find(|(k, _)| k == flag_id) {
								cat_mask |= mask;
							}
						}
					} else if id == "disable_custom" {
						disable_custom = included;
					}
				}
				FilterValue::Select { id, value } => {
					if id == "min_rating" {
						min_rating = value.parse::<i32>().unwrap_or(0);
					} else if id == "genre" && !value.is_empty() {
						query_str.push_str(&format!(" \"{}\"", value));
					} else if id == "expunged" && value == "on" {
						qs.push("f_sh", Some("on"));
					}
				}
				FilterValue::Text { id, value } if !value.is_empty() => {
					match id.as_str() {
						"tags" => tag_filter = Some(value),
						"author" => {
							// clicked from author field: search both artist and group (OR via ~)
							query_str
								.push_str(&format!(" ~\"artist:{}\" ~\"group:{}\"", value, value));
						}
						"artist" => query_str.push_str(&format!(" artist:\"{}\"", value)),
						"group" => query_str.push_str(&format!(" group:\"{}\"", value)),
						"min_pages" => min_pages = Some(value),
						"max_pages" => max_pages = Some(value),
						_ => {}
					}
				}
				_ => {}
			}
		}

		// Tags from text filter
		if let Some(tags) = tag_filter {
			for raw_tag in tags.split(',') {
				let t = raw_tag.trim();
				if t.is_empty() {
					continue;
				}
				if t.starts_with('-') {
					query_str.push_str(&format!(" -\"{}\"", t.trim_start_matches('-').trim()));
				} else if t.starts_with('~') {
					query_str.push_str(&format!(" ~\"{}\"", t.trim_start_matches('~').trim()));
				} else {
					query_str.push_str(&format!(" \"{}\"", t));
				}
			}
		}

		// Language filter from settings (OR across selected languages)
		// Note: toplist does not support language filtering, so save query before appending
		let query_str_for_toplist = query_str.clone();
		let lang_filter = get_language_filter();
		match lang_filter.as_slice() {
			[] => {}
			[single] => query_str.push_str(&format!(" \"language:{}\"", single)),
			langs => {
				for lang in langs {
					query_str.push_str(&format!(" ~\"language:{}\"", lang));
				}
			}
		}

		if !query_str.is_empty() {
			qs.push("f_search", Some(query_str.trim()));
		}

		if cats_filtered && cat_mask != ALL_CATS {
			for (flag_id, mask) in cat_flags {
				qs.push(flag_id, Some(if cat_mask & mask != 0 { "1" } else { "0" }));
			}
		}

		if min_rating > 0 {
			qs.push("f_sr", Some("on"));
			qs.push("f_srdd", Some(&(min_rating + 1).to_string()));
		}

		for param in &disable_custom {
			qs.push(param, Some("on"));
		}

		if let Some(ref min) = min_pages {
			qs.push("f_sp", Some("on"));
			qs.push("f_spf", Some(min));
		}
		if let Some(ref max) = max_pages {
			qs.push("f_sp", Some("on"));
			qs.push("f_spt", Some(max));
		}

		let cursor_id = "search";
		// toplist sorts: 2=Top Yesterday(tl=15), 3=Top Month(tl=13), 4=Top Year(tl=12), 5=Top All(tl=11)
		let toplist_tl: Option<u32> = match sort_index {
			2 => Some(15),
			3 => Some(13),
			4 => Some(12),
			5 => Some(11),
			_ => None,
		};

		if let Some(tl) = toplist_tl {
			let p = page - 1;
			let toplist_qs = if !query_str_for_toplist.trim().is_empty() {
				format!(
					"tl={tl}&p={p}&f_apply=Apply+Filter&f_search={}",
					encode_uri_component(query_str_for_toplist.trim())
				)
			} else {
				format!("tl={tl}&p={p}")
			};
			let url = format!("https://e-hentai.org/toplist.php?{toplist_qs}");
			let html = eh_get_html(&url, &cookies, USER_AGENT)?;
			let (items, has_next) = parse_toplist(&html, &base_url);
			return Ok(items_to_manga_page(items, has_next));
		}

		if page == 1 {
			clear_page_cursor(cursor_id);
		} else if let Some(gid) = get_page_cursor(cursor_id) {
			qs.push("next", Some(&gid));
		}

		let url = if sort_index == 1 {
			format!("{base_url}/?f_srdd=5&f_sr=on&{qs}")
		} else {
			format!("{base_url}/?{qs}")
		};

		let html = eh_get_html(&url, &cookies, USER_AGENT)?;

		let (items, has_next, last_gid) = parse_gallery_list(&html, &base_url);
		if let Some(gid) = last_gid {
			set_page_cursor(cursor_id, &gid);
		}
		Ok(items_to_manga_page(items, has_next))
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if !needs_details && !needs_chapters {
			return Ok(manga);
		}

		let url = &manga.key;
		let url = rewrite_domain(url);
		let cookies = build_cookie_header();

		let html = eh_get_html(&url, &cookies, USER_AGENT)?;

		let gallery = if needs_details || needs_chapters {
			parse_gallery_detail(&html, &url)
		} else {
			None
		};

		if needs_details && let Some(ref g) = gallery {
			let updated: Manga = g.clone().into();
			manga.copy_from(updated);
		}

		if needs_chapters {
			let scanlators = gallery.as_ref().and_then(|g| {
				if g.language.is_empty() {
					return None;
				}
				let lang = if g.translated {
					format!("{} (Translated)", g.language)
				} else {
					g.language.clone()
				};
				Some(vec![lang])
			});

			let date_uploaded = gallery.as_ref().and_then(|g| {
				if g.posted.is_empty() {
					return None;
				}
				parse_date(&g.posted, "yyyy-MM-dd HH:mm")
			});

			let chapter = Chapter {
				key: manga.key.clone(),
				title: gallery.as_ref().and_then(|g| {
					if g.category.is_empty() {
						None
					} else {
						Some(g.category.clone())
					}
				}),
				chapter_number: Some(1.0),
				date_uploaded,
				url: Some(url.clone()),
				scanlators,
				..Default::default()
			};
			manga.chapters = Some(vec![chapter]);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let gallery_url = rewrite_domain(&chapter.key);
		let cookies = build_cookie_header();

		let mut viewer_urls: Vec<String> = Vec::new();
		let mut next_url: Option<String> = Some(gallery_url.clone());

		while let Some(ref url) = next_url.clone() {
			let html = eh_get_html(url, &cookies, USER_AGENT)?;
			viewer_urls.extend(
				parse_gallery_pages(&html)
					.into_iter()
					.map(|u| rewrite_domain(&u)),
			);
			next_url = parse_next_gallery_page(&html).map(|u| rewrite_domain(&u));
		}

		if viewer_urls.is_empty() {
			return Err(AidokuError::Message("No pages found".into()));
		}

		let first_fetch_url = viewer_urls[0]
			.split('#')
			.next()
			.unwrap_or(&viewer_urls[0])
			.to_string();

		let first_html = eh_get_html(&first_fetch_url, &cookies, USER_AGENT).ok();

		let mpv_info = first_html.as_ref().and_then(parse_mpv_info);
		let showkey = if mpv_info.is_none() {
			first_html
				.as_ref()
				.and_then(parse_showkey)
				.unwrap_or_default()
		} else {
			String::new()
		};

		// MPV viewer URL format: https://e-hentai.org/mpv/{gid}/{token}/
		// segments: ["https:", "", "e-hentai.org", "mpv", "{gid}", "{token}"]
		let mpv_gid = if mpv_info.is_some() {
			let base = first_fetch_url.trim_end_matches('/');
			let segments: Vec<&str> = base.split('/').collect();
			segments
				.iter()
				.rev()
				.nth(1)
				.copied()
				.filter(|s| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty())
				.map(|s| s.to_string())
		} else {
			None
		};

		let pages = viewer_urls
			.into_iter()
			.enumerate()
			.map(|(idx, viewer_url)| {
				let mut context = PageContext::new();

				if let Some((ref mpvkey, ref image_keys)) = mpv_info {
					let gid = mpv_gid.clone().unwrap_or_default();
					let page = (idx as u32) + 1;

					context.insert("mode".into(), "mpv".into());
					context.insert("mpvkey".into(), mpvkey.clone());
					context.insert("gid".into(), gid);
					context.insert("page".into(), page.to_string());
					if let Some(key) = image_keys.get(idx) {
						context.insert("imgkey".into(), key.clone());
					}
				} else {
					context.insert("mode".into(), "showpage".into());
					context.insert("showkey".into(), showkey.clone());
					if let Some(imgkey) = parse_imgkey_from_viewer_url(&viewer_url) {
						context.insert("imgkey".into(), imgkey);
					}
					if let Some((gid, page)) = parse_gid_page_from_viewer_url(&viewer_url) {
						context.insert("gid".into(), gid);
						context.insert("page".into(), page.to_string());
					}
				}

				context.insert("viewer_url".into(), viewer_url.clone());
				Page {
					content: PageContent::url_context(viewer_url, context),
					..Default::default()
				}
			})
			.collect();

		Ok(pages)
	}
}

impl ListingProvider for EHentai {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let base_url = get_base_url();
		let cookies = build_cookie_header();

		let toplist_tl = match listing.id.as_str() {
			"top_yesterday" => Some(15u32),
			"top_month" => Some(13u32),
			"top_year" => Some(12u32),
			"top_all" => Some(11u32),
			_ => None,
		};

		if let Some(tl) = toplist_tl {
			let p = page - 1;
			let url = format!("https://e-hentai.org/toplist.php?tl={tl}&p={p}");
			let html = eh_get_html(&url, &cookies, USER_AGENT)?;
			let (items, has_next) = parse_toplist(&html, &base_url);
			return Ok(items_to_manga_page(items, has_next));
		}

		// For latest/popular: cursor-based pagination using stored last GID
		let cursor_id = listing.id.as_str();
		if page == 1 {
			clear_page_cursor(cursor_id);
		}

		let next_param = get_page_cursor(cursor_id)
			.filter(|_| page > 1)
			.map(|gid| format!("&next={gid}"))
			.unwrap_or_default();

		// Build language filter query string
		let lang_filter = get_language_filter();
		let lang_param = match lang_filter.as_slice() {
			[] => String::new(),
			[single] => format!(
				"&advsearch=1&f_apply=Apply+Filter&f_search={}",
				encode_uri_component(format!("\"language:{}\"", single))
			),
			langs => {
				let q: String = langs
					.iter()
					.map(|l| format!("~\"language:{}\"", l))
					.collect::<Vec<_>>()
					.join(" ");
				format!(
					"&advsearch=1&f_apply=Apply+Filter&f_search={}",
					encode_uri_component(&q)
				)
			}
		};

		let url = match listing.id.as_str() {
			"latest" => {
				if next_param.is_empty() && lang_param.is_empty() {
					format!("{base_url}/")
				} else {
					format!(
						"{base_url}/?{}{}",
						next_param.trim_start_matches('&'),
						lang_param
					)
				}
			}
			"popular" => format!("{base_url}/popular"),
			"watched" => {
				if next_param.is_empty() && lang_param.is_empty() {
					format!("{base_url}/watched")
				} else {
					format!(
						"{base_url}/watched?{}{}",
						next_param.trim_start_matches('&'),
						lang_param
					)
				}
			}
			_ => return Err(AidokuError::Unimplemented),
		};

		let html = eh_get_html(&url, &cookies, USER_AGENT)?;

		let (items, has_next, last_gid) = parse_gallery_list(&html, &base_url);
		if let Some(gid) = last_gid {
			set_page_cursor(cursor_id, &gid);
		}
		Ok(items_to_manga_page(items, has_next))
	}
}

impl DynamicListings for EHentai {
	fn get_dynamic_listings(&self) -> Result<Vec<Listing>> {
		let mut listings = Vec::new();

		if !get_ipb_member_id().is_empty() && !get_ipb_pass_hash().is_empty() {
			listings.push(Listing {
				id: "watched".into(),
				name: "Watched".into(),
				..Default::default()
			});
		}

		listings.extend([
			Listing {
				id: "latest".into(),
				name: "Latest".into(),
				..Default::default()
			},
			Listing {
				id: "popular".into(),
				name: "Popular".into(),
				..Default::default()
			},
			Listing {
				id: "top_yesterday".into(),
				name: "Top Yesterday".into(),
				..Default::default()
			},
			Listing {
				id: "top_month".into(),
				name: "Top Month".into(),
				..Default::default()
			},
			Listing {
				id: "top_year".into(),
				name: "Top Year".into(),
				..Default::default()
			},
			Listing {
				id: "top_all".into(),
				name: "Top All Time".into(),
				..Default::default()
			},
		]);

		Ok(listings)
	}
}

impl ImageRequestProvider for EHentai {
	fn get_image_request(&self, url: String, context: Option<PageContext>) -> Result<Request> {
		let cookies = build_cookie_header();
		let base_url = get_base_url();

		if let Some(ctx) = context {
			let mode = ctx.get("mode").cloned().unwrap_or_default();
			let imgkey = ctx.get("imgkey").cloned().unwrap_or_default();
			let gid = ctx.get("gid").cloned().unwrap_or_default();
			let page_str = ctx.get("page").cloned().unwrap_or_default();
			let viewer_url = ctx
				.get("viewer_url")
				.cloned()
				.unwrap_or_else(|| url.clone());
			let page: u32 = page_str.parse().unwrap_or(1);

			if !imgkey.is_empty() && !gid.is_empty() {
				if mode == "mpv" {
					let mpvkey = ctx.get("mpvkey").cloned().unwrap_or_default();
					if !mpvkey.is_empty()
						&& let Some((img_url, nl_val)) =
							api_imagedispatch(&gid, &imgkey, page, &mpvkey, None, &cookies)
					{
						if img_url.contains("509.gif") || img_url.contains("509") {
							if let Some(ref nl) = nl_val
								&& let Some((retry_url, _)) = api_imagedispatch(
									&gid,
									&imgkey,
									page,
									&mpvkey,
									Some(nl),
									&cookies,
								) {
								return Ok(Request::get(retry_url)?
									.header("Cookie", &cookies)
									.header("User-Agent", USER_AGENT)
									.header("Referer", &base_url));
							}
						} else {
							return Ok(Request::get(img_url)?
								.header("Cookie", &cookies)
								.header("User-Agent", USER_AGENT)
								.header("Referer", &base_url));
						}
					}
				} else {
					let showkey = ctx.get("showkey").cloned().unwrap_or_default();
					if !showkey.is_empty()
						&& let Some((img_url, nl_val)) =
							api_showpage(&gid, &imgkey, page, &showkey, None, &cookies)
					{
						if img_url.contains("509.gif") || img_url.contains("509") {
							if let Some(ref nl) = nl_val
								&& let Some((retry_url, _)) =
									api_showpage(&gid, &imgkey, page, &showkey, Some(nl), &cookies)
							{
								return Ok(Request::get(retry_url)?
									.header("Cookie", &cookies)
									.header("User-Agent", USER_AGENT)
									.header("Referer", &base_url));
							}
						} else {
							return Ok(Request::get(img_url)?
								.header("Cookie", &cookies)
								.header("User-Agent", USER_AGENT)
								.header("Referer", &base_url));
						}
					}
				}

				// API failed: HTML viewer page fallback
				if let Ok(html) = eh_get_html(&viewer_url, &cookies, USER_AGENT) {
					let img_url = parse_image_page(&html).unwrap_or_default();
					if !img_url.is_empty() && !img_url.contains("509") {
						return Ok(Request::get(img_url)?
							.header("Cookie", &cookies)
							.header("User-Agent", USER_AGENT)
							.header("Referer", &base_url));
					}
					if let Some(nl) = parse_nl_value(&html) {
						let retry_viewer = if viewer_url.contains('?') {
							format!("{}&nl={}", viewer_url, nl)
						} else {
							format!("{}?nl={}", viewer_url, nl)
						};
						if let Ok(retry_html) = eh_get_html(&retry_viewer, &cookies, USER_AGENT) {
							let retry_img = parse_image_page(&retry_html).unwrap_or_default();
							if !retry_img.is_empty() {
								return Ok(Request::get(retry_img)?
									.header("Cookie", &cookies)
									.header("User-Agent", USER_AGENT)
									.header("Referer", &base_url));
							}
						}
					}
				}
			}
		}

		Ok(Request::get(url)?
			.header("Cookie", &cookies)
			.header("User-Agent", USER_AGENT)
			.header("Referer", &base_url))
	}
}

impl DeepLinkHandler for EHentai {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if !url.contains("e-hentai.org/g/") && !url.contains("exhentai.org/g/") {
			return Ok(None);
		}
		let normalized = normalize_gallery_url(&url);
		Ok(Some(DeepLinkResult::Manga { key: normalized }))
	}
}

register_source!(
	EHentai,
	Home,
	ListingProvider,
	DeepLinkHandler,
	ImageRequestProvider,
	DynamicListings
);
