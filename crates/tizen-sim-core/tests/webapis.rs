//! The Samsung Product API Shims (`webapis.*`), loaded through the virtual
//! `$WEBAPIS/webapis/webapis.js` script and exercised through a Session on a
//! real Engine.

mod support;

use serde_json::json;
use support::{STARTUP_SECS, eval_callback, eventually_equals, expect_event, start, start_with};
use tizen_sim_core::session::{Lifecycle, SessionEvent};

const PRODUCTINFO_PRIVILEGE: &str = "http://developer.samsung.com/privilege/productinfo";
const NETWORK_PRIVILEGE: &str = "http://developer.samsung.com/privilege/network.public";

/// Runs `body` and returns the name of what it throws, or `"no error"`.
fn attempt(body: &str) -> String {
    format!(
        "(function () {{ try {{ {body}; return 'no error'; }} catch (e) {{ return e.name; }} }})()"
    )
}

#[tokio::test]
async fn webapis_script_resolves_from_the_root_and_from_nested_pages() {
    let t = start("webapis");
    t.session.wait_until_loaded().await.unwrap();
    let saw = t
        .session
        .evaluate("window.firstScriptSawWebapis")
        .await
        .unwrap();
    assert_eq!(saw, "object");

    // pages/nested/page.html refers to the script relative to itself.
    t.session
        .evaluate("location.href = 'pages/nested/page.html'; 0")
        .await
        .unwrap();
    eventually_equals(
        &t.session,
        "location.pathname.indexOf('nested/page.html') >= 0 && window.firstScriptSawWebapis",
        json!("object"),
    )
    .await;
    assert_eq!(
        t.session
            .evaluate("typeof webapis.productinfo.getDuid")
            .await
            .unwrap(),
        "function"
    );
    t.session.stop().await;
}

/// The TV provides `webapis` only to pages that load `$WEBAPIS/webapis/webapis.js`
/// (samsung-product-api-references.html), unlike `tizen`.
#[tokio::test]
async fn pages_without_the_script_have_no_webapis() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    assert_eq!(
        t.session.evaluate("typeof webapis").await.unwrap(),
        "undefined"
    );
    t.session.stop().await;
}

/// productinfo-api.html.
#[tokio::test]
async fn productinfo_reports_version_profile_values() {
    let t = start("webapis");
    t.session.wait_until_loaded().await.unwrap();
    let values = t
        .session
        .evaluate(
            "(function () { var p = webapis.productinfo; return { \
               firmware: p.getFirmware(), duid: p.getDuid(), model: p.getModel(), \
               modelCode: p.getModelCode(), realModel: p.getRealModel(), \
               udPanel: p.isUdPanelSupported(), serverType: p.getSmartTVServerType(), \
               operating: p.ProductInfoSiServerType.SI_TYPE_OPERATIING_SERVER, \
               country: p.getSystemConfig(p.ProductInfoConfigKey.CONFIG_KEY_SERVICE_COUNTRY) \
             }; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        values,
        json!({
            "firmware": support::device_value("firmware"),
            "duid": support::device_value("duid"),
            "model": support::device_value("productModel"),
            "modelCode": support::device_value("modelCode"),
            "realModel": support::device_value("realModel"),
            "udPanel": true,
            "serverType": 0,
            "operating": 0,
            "country": support::device_value("serviceCountry"),
        })
    );

    // CONFIG_KEY_SERVICE_COUNTRY is read-only; anything else isn't a key.
    let errors = t
        .session
        .evaluate(&format!(
            "[{}, {}]",
            attempt(
                "webapis.productinfo.setSystemConfig( \
                   webapis.productinfo.ProductInfoConfigKey.CONFIG_KEY_SERVICE_COUNTRY, 'KOR')"
            ),
            attempt("webapis.productinfo.getSystemConfig(42)"),
        ))
        .await
        .unwrap();
    assert_eq!(errors, json!(["NotSupportedError", "InvalidValuesError"]));

    let changed = eval_callback(
        &t.session,
        "var p = webapis.productinfo, key = p.ProductInfoConfigKey.CONFIG_KEY_DATA_SERVICE; \
         p.addSystemConfigChangeListener(key, function (k) { \
           done([k, p.getSystemConfig(key)]); }); \
         p.setSystemConfig(key, '1');",
    )
    .await;
    assert_eq!(changed, json!([0, "1"]));
    t.session.stop().await;
}

/// tvinfo-api.html: no method a TV App can call needs a privilege.
#[tokio::test]
async fn tvinfo_reports_version_profile_values() {
    let t = start("webapis-unprivileged");
    t.session.wait_until_loaded().await.unwrap();
    let values = t
        .session
        .evaluate(
            "(function () { var t = webapis.tvinfo; return [ \
               t.getMenuValue(t.TvInfoMenuKey.CAPTION_ONOFF_KEY), \
               t.getMenuValue(t.TvInfoMenuKey.PRIMARY_AUDIO_LANGUAGE_KEY), \
               t.isTvsPicSizeResized(), \
               typeof t.addCaptionChangeListener(t.TvInfoMenuKey.CAPTION_ONOFF_KEY, function () {}) \
             ]; })()",
        )
        .await
        .unwrap();
    assert_eq!(values, json!([0, "ENG", false, "number"]));
    // A TvInfoKey since Tizen 6.5.
    if support::has_api("TvInfoKey.SMART_RC_BATTERY") {
        assert_eq!(
            t.session
                .evaluate("webapis.tvinfo.getTvInfoValue('SMART_RC_BATTERY')")
                .await
                .unwrap(),
            json!(80)
        );
    }

    let errors = t
        .session
        .evaluate(&format!(
            "[{}, {}]",
            attempt("webapis.tvinfo.getMenuValue('NOT_A_KEY')"),
            attempt("webapis.tvinfo.removeCaptionChangeListener(12345)"),
        ))
        .await
        .unwrap();
    assert_eq!(errors, json!(["TypeMismatchError", "InvalidValuesError"]));
    t.session.stop().await;
}

/// avinfo-api.html: no method names a privilege.
#[tokio::test]
async fn avinfo_reports_version_profile_values() {
    let t = start("webapis-unprivileged");
    t.session.wait_until_loaded().await.unwrap();
    let values = t
        .session
        .evaluate(
            "(function () { var a = webapis.avinfo; return [ \
               a.getVersion() === webapis.tvinfo.getVersion(), a.isHdrTvSupport(), \
               a.AvInfoDigitalCompMode.DOLBY_DIGITAL_COMP_MODE_LINE, \
               a.AvInfoDigitalCompMode.DOLBY_DIGITAL_COMP_MODE_RF \
             ]; })()",
        )
        .await
        .unwrap();
    assert_eq!(values, json!([true, true, 0, 1]));
    // "This feature doesn't be supported since 2016", before every Version
    // Profile.
    assert_eq!(
        t.session
            .evaluate(&attempt("webapis.avinfo.getDolbyDigitalCompMode()"))
            .await
            .unwrap(),
        "NotSupportedError"
    );
    // Since 6.0, Hotel TV only.
    if support::has_api("webapis.avinfo.setSubAmpMode") {
        let errors = t
            .session
            .evaluate(&format!(
                "[{}, {}, {}]",
                attempt("webapis.avinfo.setSubAmpMode(1)"),
                attempt("webapis.avinfo.setSubAmpMode('LOUD')"),
                attempt("webapis.avinfo.setSubAmpMode('EXT_AUDIO_TV')"),
            ))
            .await
            .unwrap();
        assert_eq!(
            errors,
            json!([
                "TypeMismatchError",
                "InvalidValuesError",
                "NotSupportedError"
            ])
        );
    }
    // Since 6.5: settable, and the setting sticks.
    if support::has_api("webapis.avinfo.getEnergySaving") {
        let values = t
            .session
            .evaluate(
                "(function () { var a = webapis.avinfo; \
                   var before = [a.getEnergySaving(), a.getEcoSensor()]; \
                   a.setEnergySaving('HIGH'); a.setEcoSensor('ON'); \
                   return before.concat([a.getEnergySaving(), a.getEcoSensor()]); })()",
            )
            .await
            .unwrap();
        assert_eq!(values, json!(["OFF", "OFF", "HIGH", "ON"]));
        let errors = t
            .session
            .evaluate(&format!(
                "[{}, {}, {}, {}]",
                attempt("webapis.avinfo.setEnergySaving(2)"),
                attempt("webapis.avinfo.setEnergySaving('MAX')"),
                attempt("webapis.avinfo.setEcoSensor(true)"),
                attempt("webapis.avinfo.setEcoSensor('AUTO')"),
            ))
            .await
            .unwrap();
        assert_eq!(
            errors,
            json!([
                "TypeMismatchError",
                "InvalidValuesError",
                "TypeMismatchError",
                "InvalidValuesError"
            ])
        );
    }
    t.session.stop().await;
}

/// avinfo-api.html: getVersion, isHdrTvSupport and the Energy Saving and Eco
/// Sensor methods throw NotSupportedError "for non Samsung products", which
/// a licensed brand marks (as for productinfo.getLicensedBrand).
#[tokio::test]
async fn avinfo_is_not_supported_on_licensed_products() {
    let t = start_with("webapis-unprivileged", |c| {
        c.device.insert("licensedBrand".into(), json!("Acme"));
    });
    t.session.wait_until_loaded().await.unwrap();
    let mut calls = vec!["getVersion()", "isHdrTvSupport()"];
    if support::has_api("webapis.avinfo.getEnergySaving") {
        calls.extend([
            "getEnergySaving()",
            "setEnergySaving('LOW')",
            "getEcoSensor()",
            "setEcoSensor('ON')",
        ]);
    }
    let attempts: Vec<String> = calls
        .iter()
        .map(|call| attempt(&format!("webapis.avinfo.{call}")))
        .collect();
    let errors = t
        .session
        .evaluate(&format!("[{}]", attempts.join(", ")))
        .await
        .unwrap();
    assert_eq!(errors, json!(vec!["NotSupportedError"; calls.len()]));
    t.session.stop().await;
}

/// network-api.html.
#[tokio::test]
async fn network_reports_version_profile_values() {
    let t = start("webapis");
    t.session.wait_until_loaded().await.unwrap();
    let values = t
        .session
        .evaluate(
            "(function () { var n = webapis.network; return { \
               type: n.getActiveConnectionType(), \
               ethernet: n.NetworkActiveConnectionType.ETHERNET, \
               gateway: n.isConnectedToGateway(), ip: n.getIp(), \
               subnet: n.getSubnetMask(), router: n.getGateway(), dns: n.getDns(), \
               mac: n.getMac(), ipMode: n.getIpMode(), \
               listener: typeof n.addNetworkStateChangeListener(function () {}) \
             }; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        values,
        json!({
            "type": 3,
            "ethernet": 3,
            "gateway": true,
            "ip": "192.168.0.11",
            "subnet": "255.255.255.0",
            "router": "192.168.0.1",
            "dns": "192.168.0.1",
            "mac": "00:00:5E:00:53:01",
            "ipMode": 2,
            "listener": "number",
        })
    );
    // On Ethernet there is no Wi-Fi to describe.
    assert_eq!(
        t.session
            .evaluate(&attempt("webapis.network.getWiFiSsid()"))
            .await
            .unwrap(),
        "InvalidStateError"
    );
    t.session.stop().await;
}

#[tokio::test]
async fn per_session_overrides_replace_version_profile_values() {
    let t = start_with("webapis", |c| {
        c.device.insert("duid".into(), json!("CUSTOM-DUID"));
        c.device.insert("networkType".into(), json!("WIFI"));
        c.device.insert("wifiSsid".into(), json!("Living room"));
    });
    t.session.wait_until_loaded().await.unwrap();
    let values = t
        .session
        .evaluate(
            "[webapis.productinfo.getDuid(), webapis.network.getActiveConnectionType(), \
              webapis.network.getWiFiSsid()]",
        )
        .await
        .unwrap();
    assert_eq!(values, json!(["CUSTOM-DUID", 1, "Living room"]));

    // The same value backs tizen.systeminfo.
    let network = eval_callback(
        &t.session,
        "tizen.systeminfo.getPropertyValue('NETWORK', function (n) { done(n.networkType); });",
    )
    .await;
    assert_eq!(network, "WIFI");
    t.session.stop().await;
}

#[tokio::test]
async fn a_disconnected_tv_reports_no_network() {
    let t = start_with("webapis", |c| {
        c.device.insert("networkType".into(), json!("NONE"));
    });
    t.session.wait_until_loaded().await.unwrap();
    let values = t
        .session
        .evaluate(&format!(
            "[webapis.network.getActiveConnectionType(), webapis.network.isConnectedToGateway(), {}]",
            attempt("webapis.network.getIp()")
        ))
        .await
        .unwrap();
    assert_eq!(values, json!([0, false, "InvalidStateError"]));
    t.session.stop().await;
}

#[tokio::test]
async fn an_unknown_override_fails_the_session() {
    let t = start_with("webapis", |c| {
        c.device.insert("duidd".into(), json!("typo"));
    });
    let error = t.session.wait_until_loaded().await.unwrap_err().to_string();
    assert!(error.contains("duidd"), "{error}");
}

#[tokio::test]
async fn undeclared_samsung_privileges_throw_security_error() {
    let mut t = start("webapis-unprivileged");
    t.session.wait_until_loaded().await.unwrap();
    let thrown = t
        .session
        .evaluate(
            "(function () { \
               function code(f) { try { f(); return 'no error'; } catch (e) { return [e.name, e.code]; } } \
               return [ \
                 code(function () { webapis.productinfo.getDuid(); }), \
                 code(function () { webapis.network.getIp(); }) \
               ]; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        thrown,
        json!([["SecurityError", 18], ["SecurityError", 18]])
    );
    // isDisplayRotatorSupported (since 5.5) needs no privilege (productinfo-api.html).
    if support::has_api("webapis.productinfo.isDisplayRotatorSupported") {
        assert_eq!(
            t.session
                .evaluate("typeof webapis.productinfo.isDisplayRotatorSupported()")
                .await
                .unwrap(),
            "boolean"
        );
    }
    for privilege in [PRODUCTINFO_PRIVILEGE, NETWORK_PRIVILEGE] {
        expect_event(&mut t.events, 10, |e| {
            matches!(e, SessionEvent::Diagnostic(d)
                if d.code == "privilege-not-declared" && d.message.contains(privilege))
        })
        .await;
    }
    t.session.stop().await;
}

#[tokio::test]
async fn lenient_mode_logs_undeclared_samsung_privileges() {
    let mut t = start_with("webapis-unprivileged", |c| c.lenient = true);
    expect_event(&mut t.events, STARTUP_SECS, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::AppLoaded { .. }))
    })
    .await;
    let duid = t
        .session
        .evaluate("webapis.productinfo.getDuid()")
        .await
        .unwrap();
    assert_eq!(duid, "TIZENSIMDUID00001");
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(PRODUCTINFO_PRIVILEGE))
    })
    .await;
    t.session.stop().await;
}

/// Members of `tizen` and `webapis` newer than the Version Profile, or
/// removed before it, are absent from the page; the others are there
/// (`data/apis.toml`, from each member's "Since" in its API reference).
#[tokio::test]
async fn api_members_follow_the_version_profile() {
    let t = start("webapis");
    t.session.wait_until_loaded().await.unwrap();
    for name in tizen_sim_core::apis::ApiCatalog::bundled().names() {
        let (owner, member) = name.rsplit_once('.').unwrap();
        // Whether the page has the member.
        let probe = match owner {
            // A string that isn't a SystemInfoPropertyId throws TypeMismatchError.
            "SystemInfoPropertyId" => format!(
                "(function () {{ try {{ tizen.systeminfo.getPropertyValue('{member}', \
                   function () {{}}, function () {{}}); return true; }} \
                 catch (e) {{ return e.name !== 'TypeMismatchError'; }} }})()"
            ),
            "TvInfoKey" => format!(
                "(function () {{ try {{ webapis.tvinfo.getTvInfoValue('{member}'); \
                   return true; }} catch (e) {{ return false; }} }})()"
            ),
            "TvInfoMenuKey" => format!("'{member}' in webapis.tvinfo.TvInfoMenuKey"),
            path => format!("'{member}' in {path}"),
        };
        assert_eq!(
            t.session.evaluate(&probe).await.unwrap(),
            json!(support::has_api(name)),
            "{name} on Tizen {}",
            support::profile().version
        );
    }
    t.session.stop().await;
}
