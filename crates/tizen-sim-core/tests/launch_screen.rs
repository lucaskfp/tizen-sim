//! The App's launch screen, at the Session seam: shown from launch and hidden
//! when `ready_when` says (TD reference/config-xml.md, "Tizen launch screen").
//! The `launch-screen` fixture is copied and given the `<tizen:launch_screen>`
//! a test needs.

mod support;

use serde_json::{Value, json};
use std::time::Duration;
use support::{STARTUP_SECS, TestSession, copy_fixture, eventually_equals, pixel, start_folder};

const SHOWN: &str = "!!document.querySelector('[data-tizen-sim=\"launch-screen\"]')";
const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 128, 0];
const BLUE: [u8; 3] = [0, 0, 255];

/// How long the slow stand-in server takes to answer.
const SLOW: Duration = Duration::from_secs(3);

/// A stand-in server whose image answers after `SLOW`, returning its URL.
async fn slow_image() -> String {
    let router = axum::Router::new().route(
        "/slow.svg",
        axum::routing::get(|| async {
            tokio::time::sleep(SLOW).await;
            (
                [("content-type", "image/svg+xml")],
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#,
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/slow.svg", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    url
}

/// A launched copy of the fixture with a red launch screen showing `icon.svg`.
/// The copy is returned too, as the Session serves from it.
async fn launch(ready_when: &str) -> (tempfile::TempDir, TestSession) {
    launch_with(ready_when, false).await
}

/// Like `launch`; with `slow`, the page's load waits for a slow image.
async fn launch_with(ready_when: &str, slow: bool) -> (tempfile::TempDir, TestSession) {
    let app = copy_fixture("launch-screen");
    if slow {
        let index = app.path().join("index.html");
        let html = std::fs::read_to_string(&index).unwrap();
        let img = format!(r#"<img src="{}">"#, slow_image().await);
        std::fs::write(&index, html.replace("<!-- slow image -->", &img)).unwrap();
    }
    let config = app.path().join("config.xml");
    let xml = std::fs::read_to_string(&config).unwrap();
    let element = format!(
        r##"<access origin="*"/>
    <tizen:launch_screen ready_when="{ready_when}">
        <tizen:ls_default background_image="" background_color="#ff0000" image="icon.svg" image_border="0px stretch"/>
    </tizen:launch_screen>
</widget>"##
    );
    std::fs::write(&config, xml.replace("</widget>", &element)).unwrap();
    let test = start_folder(app.path(), |_| {});
    tokio::time::timeout(
        Duration::from_secs(STARTUP_SECS),
        test.session.wait_until_loaded(),
    )
    .await
    .unwrap()
    .unwrap();
    (app, test)
}

/// The colours at the screen's top-left corner and centre.
async fn corner_and_centre(test: &TestSession) -> ([u8; 3], [u8; 3]) {
    let png = test.session.screenshot().await.unwrap();
    let reader = png::Decoder::new(std::io::Cursor::new(&png))
        .read_info()
        .unwrap();
    let (width, height) = (reader.info().width, reader.info().height);
    (pixel(&png, 10, 10), pixel(&png, width / 2, height / 2))
}

#[tokio::test]
async fn with_custom_it_stays_until_the_app_calls_screen_show() {
    let (_app, test) = launch("custom").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        test.session.evaluate(SHOWN).await.unwrap(),
        Value::Bool(true)
    );
    let (corner, centre) = corner_and_centre(&test).await;
    assert!(support::near(corner, RED), "{corner:?}");
    assert!(support::near(centre, BLUE), "{centre:?}");

    test.session.evaluate("window.screen.show()").await.unwrap();
    eventually_equals(&test.session, SHOWN, json!(false)).await;
    let (corner, _) = corner_and_centre(&test).await;
    assert!(support::near(corner, GREEN), "{corner:?}");
}

/// What the fixture saw: the launch screen at its first script, and 1 s later,
/// after the first frame but while a slow image holds back the load.
async fn seen(test: &TestSession) -> Value {
    test.session.evaluate("window.seen").await.unwrap()
}

#[tokio::test]
async fn with_first_paint_it_hides_at_the_first_frame() {
    let (_app, test) = launch_with("first-paint", true).await;
    assert_eq!(
        seen(&test).await,
        json!({ "atStart": true, "midLoad": false })
    );
    assert_eq!(
        test.session.evaluate(SHOWN).await.unwrap(),
        Value::Bool(false)
    );
}

#[tokio::test]
async fn with_complete_it_hides_once_the_page_has_loaded() {
    let (_app, test) = launch_with("complete", true).await;
    assert_eq!(
        seen(&test).await,
        json!({ "atStart": true, "midLoad": true })
    );
    eventually_equals(&test.session, SHOWN, json!(false)).await;
}

#[tokio::test]
async fn with_custom_it_outlasts_the_load() {
    let (_app, test) = launch_with("custom", true).await;
    assert_eq!(
        seen(&test).await,
        json!({ "atStart": true, "midLoad": true })
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        test.session.evaluate(SHOWN).await.unwrap(),
        Value::Bool(true)
    );
}

#[tokio::test]
async fn an_app_without_one_shows_none() {
    let test = support::start("launch-screen");
    test.session.wait_until_loaded().await.unwrap();
    assert_eq!(seen(&test).await["atStart"], json!(false));
    assert_eq!(
        test.session.evaluate(SHOWN).await.unwrap(),
        Value::Bool(false)
    );
}

#[tokio::test]
async fn it_shows_once_per_launch() {
    let (_app, test) = launch("custom").await;
    test.session.evaluate("window.screen.show()").await.unwrap();
    test.session
        .evaluate("location.href = 'page2.html'; 0")
        .await
        .unwrap();
    let page_and_shown = format!("location.pathname + ' ' + {SHOWN}");
    eventually_equals(&test.session, &page_and_shown, json!("/page2.html false")).await;

    // The Host's reload launches the App again.
    test.session.evaluate("history.back(); 0").await.unwrap();
    eventually_equals(&test.session, "location.pathname", json!("/index.html")).await;
    test.session.reload().await.unwrap();
    assert_eq!(
        test.session.evaluate(SHOWN).await.unwrap(),
        Value::Bool(true)
    );
}
