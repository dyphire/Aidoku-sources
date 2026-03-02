use crate::settings::*;
use crate::{EHentai, USER_AGENT, parser::*};
use aidoku::{
	Home, HomeComponent, HomeComponentValue, HomeLayout, HomePartialResult, Link, Listing,
	ListingKind, Manga, Result,
	alloc::{String, Vec, vec},
	helpers::uri::encode_uri_component,
	imports::{
		net::{Request, Response},
		std::send_partial_result,
	},
	prelude::*,
};

fn items_to_links(items: Vec<crate::models::EHGalleryItem>) -> Vec<Link> {
	items
		.into_iter()
		.map(|item| -> Manga { item.into() })
		.map(|m| m.into())
		.collect()
}

fn build_and_send_toplist_big(
	resp: core::result::Result<Response, impl core::fmt::Debug>,
	title: &str,
) {
	let Ok(resp) = resp else { return };
	let Ok(html) = resp.get_html() else { return };
	let (items, _) = parse_toplist(&html, "https://e-hentai.org");
	if items.is_empty() {
		return;
	}
	let entries: Vec<Manga> = items.into_iter().map(|item| item.into()).collect();
	send_partial_result(&HomePartialResult::Component(HomeComponent {
		title: Some(title.into()),
		subtitle: None,
		value: HomeComponentValue::BigScroller {
			entries,
			auto_scroll_interval: Some(6.0),
		},
	}));
}

fn build_and_send_toplist_list(
	resp: core::result::Result<Response, impl core::fmt::Debug>,
	id: &str,
	title: &str,
) {
	let Ok(resp) = resp else { return };
	let Ok(html) = resp.get_html() else { return };
	let (items, _) = parse_toplist(&html, "https://e-hentai.org");
	if items.is_empty() {
		return;
	}
	send_partial_result(&HomePartialResult::Component(HomeComponent {
		title: Some(title.into()),
		subtitle: None,
		value: HomeComponentValue::MangaList {
			ranking: true,
			page_size: Some(5),
			entries: items_to_links(items),
			listing: Some(Listing {
				id: id.into(),
				name: title.into(),
				kind: ListingKind::Default,
			}),
		},
	}));
}

fn build_and_send_gallery_scroller(
	resp: core::result::Result<Response, impl core::fmt::Debug>,
	title: &str,
	listing_id: &str,
	base_url: &str,
) {
	let Ok(resp) = resp else { return };
	let Ok(html) = resp.get_html() else { return };
	let (items, _, _) = parse_gallery_list(&html, base_url);
	if items.is_empty() {
		return;
	}
	send_partial_result(&HomePartialResult::Component(HomeComponent {
		title: Some(title.into()),
		subtitle: None,
		value: HomeComponentValue::Scroller {
			entries: items_to_links(items),
			listing: Some(Listing {
				id: listing_id.into(),
				name: title.into(),
				kind: ListingKind::Default,
			}),
		},
	}));
}

impl Home for EHentai {
	fn get_home(&self) -> Result<HomeLayout> {
		let logged_in = !get_ipb_member_id().is_empty() && !get_ipb_pass_hash().is_empty();

		let mut skeleton: Vec<HomeComponent> = vec![
			HomeComponent {
				title: Some("Top Yesterday".into()),
				subtitle: None,
				value: HomeComponentValue::empty_big_scroller(),
			},
			HomeComponent {
				title: Some("Top Month".into()),
				subtitle: None,
				value: HomeComponentValue::empty_manga_list(),
			},
			HomeComponent {
				title: Some("Top Year".into()),
				subtitle: None,
				value: HomeComponentValue::empty_manga_list(),
			},
			HomeComponent {
				title: Some("Popular".into()),
				subtitle: None,
				value: HomeComponentValue::empty_scroller(),
			},
			HomeComponent {
				title: Some("Latest".into()),
				subtitle: None,
				value: HomeComponentValue::empty_scroller(),
			},
		];

		if logged_in {
			skeleton.insert(
				3,
				HomeComponent {
					title: Some("Watched".into()),
					subtitle: None,
					value: HomeComponentValue::empty_scroller(),
				},
			);
		}

		send_partial_result(&HomePartialResult::Layout(HomeLayout {
			components: skeleton,
		}));

		let base_url = get_base_url();
		let cookies = build_cookie_header();

		// Build language filter query string (same logic as get_manga_list)
		let lang_filter = get_language_filter();
		let lang_param: String = match lang_filter.as_slice() {
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

		let make_req = |url: &str| -> Result<Request> {
			Ok(Request::get(url)?
				.header("Cookie", &cookies)
				.header("User-Agent", USER_AGENT))
		};

		// Build all requests
		let top_yesterday_req = make_req("https://e-hentai.org/toplist.php?tl=15&p=0")?;
		let top_month_req = make_req("https://e-hentai.org/toplist.php?tl=13&p=0")?;
		let top_year_req = make_req("https://e-hentai.org/toplist.php?tl=12&p=0")?;
		let popular_req = make_req(&format!("{base_url}/popular"))?;
		let latest_req = if lang_param.is_empty() {
			make_req(&base_url)?
		} else {
			make_req(&format!(
				"{base_url}/?{}",
				lang_param.trim_start_matches('&')
			))?
		};

		if logged_in {
			let watched_url = if lang_param.is_empty() {
				format!("{base_url}/watched")
			} else {
				format!("{base_url}/watched?{}", lang_param.trim_start_matches('&'))
			};
			let watched_req = make_req(&watched_url)?;
			let responses: [core::result::Result<Response, _>; 6] = Request::send_all([
				top_yesterday_req,
				top_month_req,
				top_year_req,
				watched_req,
				popular_req,
				latest_req,
			])
			.try_into()
			.expect("6 requests");

			let [r_yesterday, r_month, r_year, r_watched, r_hot, r_latest] = responses;
			build_and_send_toplist_big(r_yesterday, "Top Yesterday");
			build_and_send_toplist_list(r_month, "top_month", "Top Month");
			build_and_send_toplist_list(r_year, "top_year", "Top Year");
			build_and_send_gallery_scroller(r_watched, "Watched", "watched", &base_url);
			build_and_send_gallery_scroller(r_hot, "Popular", "popular", &base_url);
			build_and_send_gallery_scroller(r_latest, "Latest", "latest", &base_url);
		} else {
			let responses: [core::result::Result<Response, _>; 5] = Request::send_all([
				top_yesterday_req,
				top_month_req,
				top_year_req,
				popular_req,
				latest_req,
			])
			.try_into()
			.expect("5 requests");

			let [r_yesterday, r_month, r_year, r_hot, r_latest] = responses;
			build_and_send_toplist_big(r_yesterday, "Top Yesterday");
			build_and_send_toplist_list(r_month, "top_month", "Top Month");
			build_and_send_toplist_list(r_year, "top_year", "Top Year");
			build_and_send_gallery_scroller(r_hot, "Popular", "popular", &base_url);
			build_and_send_gallery_scroller(r_latest, "Latest", "latest", &base_url);
		}

		Ok(HomeLayout {
			components: Vec::new(),
		})
	}
}
