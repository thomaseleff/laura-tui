//! HTML pages in file panes, and what the web server serves: the file route, images, the fonts and
//! the logo.

use super::*;

#[test]
fn a_markdown_image_loads_through_the_file_route() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) =
        image_doc("![a\n<b>shot $$x$$](../img/s.png)\n\n![x](https://e.com/x.png)\n\na\0b\n")?;
    rig.drive(&["open", &path]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    assert!(src.starts_with("/f/"), "{html}");
    assert!(src.ends_with("/docs/../img/s.png"), "{html}");
    assert!(!src.contains("t="), "{html}");
    assert!(html.contains(r#"alt="a &lt;b&gt;shot $$x$$""#), "{html}");
    // A remote image shows its alt text; a NUL in the file can't pose as the route.
    assert_eq!(html.matches("<img").count(), 1, "{html}");
    assert!(html.contains("<p>x</p>"), "{html}");
    assert!(html.contains("a\u{FFFD}b"), "{html}");
    let resp = get(rig.port, &src.replace("/docs/../", "/"))?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains("Content-Type: image/png\r\n"), "{resp}");
    assert!(
        resp.contains("X-Content-Type-Options: nosniff\r\n"),
        "{resp}"
    );
    assert!(
        resp.contains("Content-Security-Policy: sandbox\r\n"),
        "{resp}"
    );
    assert!(resp.ends_with("\r\n\r\nPNGBYTES"), "{resp}");
    Ok(())
}

#[test]
fn an_image_name_with_a_percent_sign_loads() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("![p](../img/100%.png)\n")?;
    std::fs::write(dir.path().join("img/100%.png"), "PCTBYTES")?;
    rig.drive(&["open", &path]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    let resp = get(rig.port, &src.replace("/docs/../", "/"))?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{html}\n{resp}");
    assert!(resp.ends_with("\r\n\r\nPCTBYTES"), "{resp}");
    Ok(())
}

#[test]
fn the_file_route_serves_only_images_in_the_root() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, path) = image_doc("![shot](../img/s.png)\n")?;
    std::fs::create_dir_all(dir.path().join(".git"))?;
    for f in ["x.png", ".x.png", ".git/x.png", "notes.txt", "x.css"] {
        std::fs::write(dir.path().join(f), "x")?;
    }
    rig.drive(&["open", &path]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    // `/f/<nonce>/<the dir below the root>/`
    let base = src
        .strip_suffix("docs/../img/s.png")
        .expect(&html)
        .to_string();
    let port = rig.port;
    let status = |p: &str| get(port, &format!("{base}{p}")).map(|r| r[..12].to_string());
    assert_eq!(status("x.png")?, "HTTP/1.1 200");
    // Without the checks, each of these would reach a file.
    for p in [
        ".x.png",
        ".git/x.png",
        "docs/%2e%2e/x.png",
        "docs/..%2fx.png",
        "notes.txt",
        // A markdown pane's folder serves images only; a page's serves its stylesheets too.
        "x.css",
    ] {
        assert_eq!(status(p)?, "HTTP/1.1 404", "{p}");
    }
    let n = base.split('/').nth(2).expect("a nonce");
    let other = base.replacen(n, &"0".repeat(n.len()), 1);
    assert!(get(rig.port, &format!("{other}x.png"))?.starts_with("HTTP/1.1 404"));
    #[cfg(unix)]
    {
        let out = tempfile::NamedTempFile::with_suffix(".png")?;
        std::os::unix::fs::symlink(out.path(), dir.path().join("link.png"))?;
        assert_eq!(status("link.png")?, "HTTP/1.1 404");
        std::os::unix::fs::symlink(".git/x.png", dir.path().join("hid.png"))?;
        std::os::unix::fs::symlink("notes.txt", dir.path().join("txt.png"))?;
        for p in ["hid.png", "txt.png"] {
            assert_eq!(status(p)?, "HTTP/1.1 404", "{p}");
        }
    }
    rig.drive(&["close", "1"]);
    rig.idle(&mut ws, 2);
    assert_eq!(status("x.png")?, "HTTP/1.1 404");
    Ok(())
}

#[test]
fn a_markdown_file_in_a_hidden_folder_shows_images_beside_it() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (dir, _) = image_doc("")?;
    std::fs::create_dir_all(dir.path().join(".hid"))?;
    std::fs::write(dir.path().join(".hid/s.png"), "PNGBYTES")?;
    let path = dir.path().join(".hid/a.md");
    std::fs::write(&path, "![s](s.png)\n")?;
    rig.drive(&["open", &path.to_string_lossy()]);
    let (html, src) = img_src(&mut rig, &mut ws, 1);
    // `/f/<nonce>/s.png`: the root is the file's own folder.
    assert_eq!(src.split('/').count(), 4, "{html}");
    assert!(get(rig.port, &src)?.ends_with("\r\n\r\nPNGBYTES"), "{html}");
    Ok(())
}

/// A page with each case Laura's tag scan must get right.
const PAGE: &str = "<!DOCTYPE html>
<html>
<body>
<button id=\"b\" title=\"a>b <i>\">Go</button>
<script>let s = \"<div>\";</script>
<!-- <p> -->
<p>a < b</p><DIV>x</DIV>
<xmp><b>x</b></xmp>
<p title=it's>x</p><em>y</em>
<!--><span>z</span>
</body>
</html>
";

/// A `GET`'s body.
fn body(resp: &str) -> &str {
    resp.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

#[test]
fn an_html_pane_shows_its_page() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file("page.html", "<p>old</p>\n", PAGE)?;
    rig.drive(&["open", &path]);
    // A page doesn't take the arrows, so the hint line doesn't list them; rows do.
    let hint = |arrows: bool| {
        move |f: &Frame| {
            matches!(f, Frame::Json(v) if v["t"] == "layout" && v["focus"] == 1
                && pairs(&v["hints"]["pane"]).contains(if arrows { "↑ ↓" } else { "Click an element" }))
        }
    };
    rig.until(&mut ws, "hint without arrows", hint(false));
    let frame = page_frame(&mut rig, &mut ws, 1, |_| true);
    let url = frame["page"].as_str().unwrap_or("");
    assert!(url.starts_with("/f/"), "{frame}");
    assert!(url.contains("/page.html?v="), "{frame}");
    assert!(!url.contains("t="), "{frame}");
    let resp = get(rig.port, url)?;
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.contains("Content-Type: text/html"), "{resp}");
    assert!(
        resp.contains(
            "Content-Security-Policy: sandbox allow-scripts allow-forms; frame-ancestors 'self'\r\n"
        ),
        "{resp}"
    );
    assert!(
        resp.contains("Access-Control-Allow-Origin: *\r\n"),
        "{resp}"
    );
    let html = body(&resp);
    // The doctype stays first, so the page isn't in quirks mode.
    assert!(html.starts_with("<!DOCTYPE html>\n"), "{html}");
    for want in [
        r#"<html data-laura-line="2">"#,
        r#"<button data-laura-line="4" id="b" title="a>b <i>">"#,
        r#"<script data-laura-line="5">let s = "<div>";"#,
        "<!-- <p> -->",
        r#"<p data-laura-line="7">a < b</p><DIV data-laura-line="7">"#,
        r#"<em data-laura-line="9">"#,
        r#"<span data-laura-line="10">"#,
    ] {
        assert!(html.contains(want), "{want}\n{html}");
    }
    for not in [
        "<i data-laura-line",
        "<div data-laura-line",
        r#"<p data-laura-line="6""#,
        "<b data-laura-line",
    ] {
        assert!(!html.contains(not), "{not}\n{html}");
    }
    // `d` shows the diff as rows, and `d` again shows the page.
    let toggle = json!({"act": "toggle_diff", "pane": 1});
    rig.act(&mut ws, toggle.clone());
    // Laura sends `layout` before the pane's frame.
    rig.until(&mut ws, "hint with arrows", hint(true));
    pane_frame(&mut rig, &mut ws, 1);
    rig.act(&mut ws, toggle);
    page_frame(&mut rig, &mut ws, 1, |_| true);
    Ok(())
}

#[test]
fn a_page_keeps_lauras_script_before_unclosed_text() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let dir = tempfile::tempdir()?;
    for (i, (page, tail)) in [
        ("<p>a</p>\n<!-- x\n", "<!-- x\n"),
        (
            "<p>a</p>\n<plaintext>x\n",
            "<plaintext data-laura-line=\"2\">x\n",
        ),
        ("<p>a</p>\n<style>x\n", "<style data-laura-line=\"2\">x\n"),
        (
            "<p>a</p>\n<noscript>x\n",
            "<noscript data-laura-line=\"2\">x\n",
        ),
        (
            "<p>a</p>\n<p title=\"x>\n",
            "<p data-laura-line=\"2\" title=\"x>\n",
        ),
        (
            "<p>a</p>\n<template>x\n",
            "<template data-laura-line=\"2\">x\n",
        ),
        (
            "<p>a</p>\n<template><template>y</template>x\n",
            "<template data-laura-line=\"2\"><template>y</template>x\n",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("page{i}.html");
        let path = dir.path().join(&name);
        std::fs::write(&path, page)?;
        let path = path.to_string_lossy();
        if i == 0 {
            rig.drive(&["open", &path]);
        } else {
            rig.drive(&["open", &path, "--panel", "1"]);
        }
        let frame = page_frame(&mut rig, &mut ws, 1, |v| {
            v["page"]
                .as_str()
                .is_some_and(|u| u.contains(&format!("/{name}?")))
        });
        let resp = get(rig.port, frame["page"].as_str().unwrap_or(""))?;
        let html = body(&resp);
        assert!(html.ends_with(tail), "{page}\n{html}");
        let before = &html[..html.len() - tail.len()];
        assert!(before.contains("data-laura-cur"), "{page}\n{html}");
    }
    Ok(())
}

#[test]
fn a_missing_html_file_shows_rows_with_the_arrows() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("missing.html");
    rig.drive(&["open", &path.to_string_lossy()]);
    // No route, so the pane shows rows, which take the arrows.
    rig.until(&mut ws, "hint with arrows", |f| {
        matches!(f, Frame::Json(v) if v["t"] == "layout" && v["focus"] == 1
            && pairs(&v["hints"]["pane"]).contains("↑ ↓"))
    });
    pane_frame(&mut rig, &mut ws, 1);
    Ok(())
}

#[test]
fn a_highlight_marks_a_page() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = changed_file("page.html", "<p>old</p>\n", PAGE)?;
    rig.drive(&["open", &path]);
    rig.drive(&["highlight", "--pane", "1", "4"]);
    page_frame(&mut rig, &mut ws, 1, |v| v["highlight"] == json!([4, 4]));
    // The page scrolls to the highlight, so `d` doesn't jump to it again.
    let toggle = json!({"act": "toggle_diff", "pane": 1});
    rig.act(&mut ws, toggle.clone());
    let frame = pane_frame(&mut rig, &mut ws, 1);
    assert_eq!(frame["recenter"], false, "{frame}");
    rig.act(&mut ws, toggle);
    page_frame(&mut rig, &mut ws, 1, |_| true);
    rig.drive(&["highlight", "--off", "--pane", "1"]);
    page_frame(&mut rig, &mut ws, 1, |v| v["highlight"].is_null());
    Ok(())
}

#[test]
fn a_page_loads_its_own_assets() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let dir = served_dir()?;
    std::fs::create_dir_all(dir.path().join("site/js"))?;
    for f in [
        "site/index.html",
        "site/style.css",
        "site/js/app.mjs",
        "site/notes.txt",
    ] {
        std::fs::write(dir.path().join(f), "x")?;
    }
    rig.drive(&[
        "open",
        &dir.path().join("site/index.html").to_string_lossy(),
    ]);
    let frame = page_frame(&mut rig, &mut ws, 1, |_| true);
    let base = frame["page"]
        .as_str()
        .and_then(|u| u.split("index.html").next())
        .unwrap_or("")
        .to_string();
    let port = rig.port;
    let resp = |p: &str| get(port, &format!("{base}{p}"));
    let css = resp("style.css")?;
    assert!(css.starts_with("HTTP/1.1 200"), "{css}");
    assert!(css.contains("Content-Type: text/css\r\n"), "{css}");
    let js = resp("js/app.mjs")?;
    assert!(js.starts_with("HTTP/1.1 200"), "{js}");
    assert!(js.contains("Content-Type: text/javascript\r\n"), "{js}");
    assert!(js.contains("Access-Control-Allow-Origin: *\r\n"), "{js}");
    // A page's route serves only its folder, so no URL reaches the folder above it.
    assert_eq!(base.matches('/').count(), 3, "{base}");
    let txt = resp("notes.txt")?;
    assert!(txt.starts_with("HTTP/1.1 404"), "{txt}");
    // A page's `/style.css` comes from its opaque origin, so it doesn't get the workspace's.
    let cross = "Sec-Fetch-Site: cross-site\r\n";
    let sub = get_as(
        port,
        "/style.css",
        &format!("{cross}Sec-Fetch-Mode: no-cors\r\n"),
    )?;
    assert!(sub.starts_with("HTTP/1.1 404"), "{sub}");
    let nav = get_as(port, "/", &format!("{cross}Sec-Fetch-Mode: navigate\r\n"))?;
    assert!(nav.starts_with("HTTP/1.1 200"), "{nav}");
    let own = get(port, "/style.css")?;
    assert!(own.starts_with("HTTP/1.1 200"), "{own}");
    Ok(())
}

#[test]
fn the_fonts_are_served() -> Result<()> {
    let rig = Rig::new()?;
    for font in ["/fonts/Inter.woff2", "/fonts/JetBrainsMono.woff2"] {
        let resp = get(rig.port, font)?;
        assert!(resp.starts_with("HTTP/1.1 200"), "{font}");
        assert!(resp.contains("Content-Type: font/woff2\r\n"), "{font}");
        assert!(body(&resp).starts_with("wOF2"), "{font}");
    }
    Ok(())
}

#[test]
fn the_web_app_shows_the_laura_logo() -> Result<()> {
    let rig = Rig::new()?;
    let logo = get(rig.port, "/logo.svg")?;
    assert!(logo.starts_with("HTTP/1.1 200"), "{logo}");
    assert!(logo.contains("Content-Type: image/svg+xml\r\n"), "{logo}");
    assert!(body(&logo).starts_with("<svg"), "{logo}");
    // The tab icon is the same mark, white in dark mode.
    let icon = get(rig.port, "/favicon.svg")?;
    assert!(icon.starts_with("HTTP/1.1 200"), "{icon}");
    assert!(icon.contains("Content-Type: image/svg+xml\r\n"), "{icon}");
    assert!(body(&icon).contains("prefers-color-scheme: dark"), "{icon}");
    let old = get(rig.port, "/laura.ico")?;
    assert!(old.starts_with("HTTP/1.1 404"), "{old}");
    let page = get(rig.port, "/")?;
    assert!(page.contains("<link rel=\"icon\" href=\"favicon.svg\" type=\"image/svg+xml\">"));
    assert!(page.contains("src=\"logo.svg\" alt=\"Laura\" width=\"17\" height=\"17\""));
    Ok(())
}

#[test]
fn a_frozen_page_serves_its_snapshot() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("page.html", PAGE)?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    let acted = rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 4, "text": "q"}),
    );
    assert_eq!(acted["ok"], true, "{acted}");
    let url = page_frame(&mut rig, &mut ws, 1, |v| v["threads"][0]["line"] == 4)["page"]
        .as_str()
        .unwrap_or("")
        .to_string();
    std::fs::write(&path, "<p>new</p>\n")?;
    rig.until(
        &mut ws,
        "frozen layout",
        |f| matches!(f, Frame::Json(v) if v["t"] == "layout" && v["panes"][1]["frozen"] == true),
    );
    let old = get(rig.port, &url)?;
    assert!(
        body(&old).contains(r#"<button data-laura-line="4""#),
        "{old}"
    );
    rig.act(&mut ws, json!({"act": "refresh", "pane": 1}));
    let frame = page_frame(&mut rig, &mut ws, 1, |v| v["page"] != url.as_str());
    let new = get(rig.port, frame["page"].as_str().unwrap_or(""))?;
    assert!(
        body(&new).starts_with(r#"<p data-laura-line="1">new</p>"#),
        "{new}"
    );
    Ok(())
}

#[test]
fn a_comment_on_an_element_quotes_its_tag() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, path) = file("page.html", PAGE)?;
    rig.drive(&["open", &path]);
    rig.drive(&["ready"]);
    // A click on the button reports its tag's line.
    rig.act(&mut ws, json!({"act": "set_cursor", "pane": 1, "line": 4}));
    rig.act(&mut ws, json!({"act": "begin_comment", "pane": 1}));
    let draft = rig.draft();
    assert_eq!(draft["line"], 4, "{draft}");
    rig.act(
        &mut ws,
        json!({"act": "comment", "pane": 1, "line": 4, "text": "q"}),
    );
    let frame = page_frame(&mut rig, &mut ws, 1, |v| v["threads"][0]["line"] == 4);
    assert_eq!(frame["cursor"], 4, "{frame}");
    assert_eq!(frame["threads"][0]["notes"][0]["body"], "q", "{frame}");
    assert_eq!(frame["card"], 4, "{frame}");
    rig.act(&mut ws, json!({"act": "begin_review", "pane": 1}));
    let acted = rig.act(&mut ws, json!({"act": "submit", "pane": 1, "body": ""}));
    assert_eq!(acted["ok"], true, "{acted}");
    rig.shell_shows(r#"L4  <button id="b""#);
    Ok(())
}

#[test]
fn a_thread_names_its_block() -> Result<()> {
    let mut rig = Rig::new()?;
    let mut ws = rig.connect();
    let (_dir, md) = file("doc.md", "# Title\n\nSome prose.\n")?;
    rig.drive(&["open", &md]);
    rig.drive(&["comment", "--pane", "1", "3", "on prose"]);
    rig.until(
        &mut ws,
        "the paragraph's label",
        |f| matches!(f, Frame::Json(v) if v["id"] == 1 && v["threads"][0]["label"] == "paragraph"),
    );
    let (_dir2, page) = file("page.html", PAGE)?;
    rig.drive(&["open", &page]);
    rig.drive(&["comment", "--pane", "2", "4", "on the button"]);
    page_frame(&mut rig, &mut ws, 2, |v| {
        v["threads"][0]["label"] == "<button#b>"
    });
    // An attribute after a tab; `data-id` isn't `id`.
    let (_dir3, tabbed) = file("tabbed.html", "<p data-id=\"x\"\tclass=\"c\">x</p>\n")?;
    rig.drive(&["open", &tabbed]);
    rig.drive(&["comment", "--pane", "3", "1", "on the paragraph"]);
    page_frame(&mut rig, &mut ws, 3, |v| {
        v["threads"][0]["label"] == "<p.c>"
    });
    Ok(())
}

#[test]
fn a_silent_socket_does_not_block_a_second_connection() -> Result<()> {
    let mut rig = Rig::new()?;
    let _silent = TcpStream::connect(("127.0.0.1", rig.port))?;
    let mut ws = rig.connect();
    rig.until(&mut ws, "layout", |f| f.is("layout"));
    Ok(())
}
