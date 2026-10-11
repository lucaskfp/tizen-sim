//! `tizen.filesystem` on the App's Sandbox, exercised through a Session on a
//! real Engine. Run on each Version Profile: the API's shape differs before
//! and after Tizen 5.0.

mod support;

use serde_json::json;
use support::{eval_callback, expect_event, start};
use tizen_sim_core::session::SessionEvent;

/// The Version Profile's Tizen version as a number, e.g. 3.0.
fn tizen_version() -> f64 {
    support::profile().version.parse().unwrap()
}

/// filesystem.html: `openFile` and the other path-based methods arrive in
/// 5.0, which deprecates `resolve` (and `File`, `FileStream`); the 6.0
/// reference no longer has them.
fn has_path_api() -> bool {
    tizen_version() >= 5.0
}

fn has_file_api() -> bool {
    tizen_version() < 6.0
}

#[tokio::test]
async fn the_api_shape_follows_the_version_profile() {
    let t = start("filesystem");
    t.session.wait_until_loaded().await.unwrap();

    let shape = t
        .session
        .evaluate(
            "[typeof tizen.filesystem.resolve, typeof tizen.filesystem.openFile, \
              typeof tizen.filesystem.pathExists, typeof tizen.filesystem.listStorages, \
              typeof tizen.filesystem.maxPathLength]",
        )
        .await
        .unwrap();
    let kind = |present: bool| if present { "function" } else { "undefined" };
    assert_eq!(
        shape,
        json!([
            kind(has_file_api()),
            kind(has_path_api()),
            kind(has_path_api()),
            "function",
            "number"
        ])
    );
    t.session.stop().await;
}

/// Page code that writes `text` to the file at `path` (made if missing)
/// and calls `done('written')`, with the Version Profile's API: a
/// FileHandle from 5.0, a File's FileStream before.
fn write_text(path: &str, text: &str) -> String {
    let (dir, name) = path.rsplit_once('/').unwrap();
    if has_path_api() {
        format!(
            "var h = tizen.filesystem.openFile('{path}', 'w'); h.writeString({text:?}); h.close(); \
             done('written');"
        )
    } else {
        format!(
            "tizen.filesystem.resolve('{dir}', function (d) {{ \
               var f; try {{ f = d.resolve('{name}'); }} catch (e) {{ f = d.createFile('{name}'); }} \
               f.openStream('w', function (s) {{ s.write({text:?}); s.close(); done('written'); }}, \
                 function (e) {{ done(e.name); }}); \
             }}, function (e) {{ done(e.name); }}, 'rw');"
        )
    }
}

/// Page code that reads the file at `path` and passes its text to `done`,
/// or the name of the error.
fn read_text(path: &str) -> String {
    if has_path_api() {
        format!(
            "try {{ var h = tizen.filesystem.openFile('{path}', 'r'); var t = h.readString(); \
               h.close(); done(t); }} catch (e) {{ done(e.name); }}"
        )
    } else {
        format!(
            "tizen.filesystem.resolve('{path}', function (f) {{ \
               f.openStream('r', function (s) {{ var t = s.read(1000); s.close(); done(t); }}, \
                 function (e) {{ done(e.name); }}); \
             }}, function (e) {{ done(e.name); }}, 'r');"
        )
    }
}

#[tokio::test]
async fn written_data_reads_back_synchronously_and_survives_reloads_and_sessions() {
    let state = tempfile::tempdir().unwrap();
    let text = "Olá, TV 📺";
    let first = support::start_session("filesystem", state.path());
    first.wait_until_loaded().await.unwrap();
    for (path, content) in [
        ("wgt-private/saved/state.txt", text),
        ("wgt-private-tmp/cache.txt", "cached"),
    ] {
        if !has_path_api() {
            // resolve() only finds existing directories.
            let dir = path.rsplit_once('/').unwrap().0;
            std::fs::create_dir_all(
                tizen_sim_core::home::Home::at(".")
                    .with_state_dir(state.path().to_path_buf())
                    .sandbox_dir("Fixture013.Filesystem")
                    .join(dir),
            )
            .unwrap();
        }
        assert_eq!(
            eval_callback(&first, &write_text(path, content)).await,
            "written"
        );
    }
    assert_eq!(
        eval_callback(&first, &read_text("wgt-private/saved/state.txt")).await,
        text
    );

    first.reload().await.unwrap();
    assert_eq!(
        eval_callback(&first, &read_text("wgt-private/saved/state.txt")).await,
        text
    );
    assert_eq!(
        eval_callback(&first, &read_text("wgt-private-tmp/cache.txt")).await,
        "cached"
    );
    first.stop().await;

    let second = support::start_session("filesystem", state.path());
    second.wait_until_loaded().await.unwrap();
    assert_eq!(
        eval_callback(&second, &read_text("wgt-private/saved/state.txt")).await,
        text
    );
    // wgt-private-tmp lasts one execution cycle (filesystem.html, resolve).
    assert_eq!(
        eval_callback(&second, &read_text("wgt-private-tmp/cache.txt")).await,
        "NotFoundError"
    );
    second.stop().await;
}

const WRITE_PRIVILEGE: &str = "http://tizen.org/privilege/filesystem.write";

/// filesystem.html: writing needs filesystem.write, and from 5.0 the media
/// roots (documents, images, ...) also need mediastorage. Undeclared, they
/// throw SecurityError.
#[tokio::test]
async fn undeclared_filesystem_privileges_throw_security_error() {
    let t = start("filesystem-read-only");
    t.session.wait_until_loaded().await.unwrap();
    let mut events = t.session.events();

    let write = if has_path_api() {
        "try { tizen.filesystem.openFile('wgt-private/a.txt', 'w'); done('opened'); } \
         catch (e) { done(e.name); }"
    } else {
        "tizen.filesystem.resolve('wgt-private', function (d) { \
           try { d.createFile('a.txt'); done('created'); } catch (e) { done(e.name); } \
         }, function (e) { done('resolve ' + e.name); }, 'rw');"
    };
    assert_eq!(eval_callback(&t.session, write).await, "SecurityError");
    expect_event(&mut events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(WRITE_PRIVILEGE))
    })
    .await;

    // Reading the App's private storage needs only filesystem.read.
    assert_eq!(
        eval_callback(&t.session, &read_text("wgt-private/missing.txt")).await,
        "NotFoundError"
    );
    let media = if has_path_api() {
        "try { tizen.filesystem.pathExists('documents/a.txt'); done('read'); } \
         catch (e) { done(e.name); }"
    } else {
        "try { tizen.filesystem.resolve('documents', function () { done('read'); }, \
           function (e) { done(e.name); }, 'r'); } catch (e) { done(e.name); }"
    };
    let expected = if tizen_version() >= 5.0 {
        "SecurityError"
    } else {
        "read"
    };
    assert_eq!(eval_callback(&t.session, media).await, expected);
    t.session.stop().await;
}

#[tokio::test]
async fn lenient_mode_allows_undeclared_writes_but_raises_the_diagnostic() {
    let t = support::start_with("filesystem-read-only", |config| config.lenient = true);
    t.session.wait_until_loaded().await.unwrap();
    let mut events = t.session.events();
    std::fs::create_dir_all(
        tizen_sim_core::home::Home::at(".")
            .with_state_dir(t.state.path().to_path_buf())
            .sandbox_dir("Fixture014.ReadOnly")
            .join("wgt-private"),
    )
    .unwrap();
    assert_eq!(
        eval_callback(&t.session, &write_text("wgt-private/a.txt", "allowed")).await,
        "written"
    );
    expect_event(&mut events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(WRITE_PRIVILEGE))
    })
    .await;
    t.session.stop().await;
}

/// The console messages the Session shows within a moment.
async fn console_texts(events: &mut tizen_sim_core::session::Events) -> Vec<String> {
    let mut texts = vec![];
    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_millis(300), events.next()).await
    {
        if let SessionEvent::Console(message) = event {
            texts.push(message.text);
        }
    }
    texts
}

/// The 5.0 API (filesystem.html, 9.0 reference): FileHandles, and the
/// path-based methods reporting the paths their reference describes.
#[tokio::test]
async fn files_are_handled_by_path_since_tizen_5() {
    if !has_path_api() {
        return;
    }
    let t = start("filesystem");
    t.session.wait_until_loaded().await.unwrap();
    let mut events = t.session.events();

    // The reference's seek example, then an append.
    let handle = eval_callback(
        &t.session,
        "var fs = tizen.filesystem; \
         var w = fs.openFile('documents/dir/data.bin', 'w'); \
         w.writeData(new Uint8Array([11, 21, 31, 44, 55, 66, 71, 81, 91])); w.close(); \
         var r = fs.openFile('documents/dir/data.bin', 'r'); \
         var first = Array.prototype.slice.call(r.readData(3)); \
         var end = r.seek(-3, 'END'); \
         var last = Array.prototype.slice.call(r.readData(3)); \
         var a = fs.openFile('documents/dir/data.bin', 'a'); a.writeData(new Uint8Array([1])); a.close(); \
         r.seek(0); var all = r.readData().length; r.close(); \
         var closed; try { r.close(); } catch (e) { closed = e.name; } \
         done([first, end, last, all, closed]);",
    )
    .await;
    assert_eq!(
        handle,
        json!([[11, 21, 31], 6, [71, 81, 91], 10, "IOError"])
    );

    // NonBlocking work runs in call order; close() finishes it first.
    let non_blocking = eval_callback(
        &t.session,
        "var log = []; var h = tizen.filesystem.openFile('documents/dir/log.txt', 'rwo'); \
         h.writeStringNonBlocking('Lorem ipsum...', function (n) { log.push('wrote ' + n); }); \
         h.seekNonBlocking(-3, function (p) { log.push('at ' + p); }, null, 'END'); \
         h.writeStringNonBlocking('!', function (n) { log.push('wrote ' + n); }); \
         h.close(); \
         var text = tizen.filesystem.openFile('documents/dir/log.txt', 'r').readString(); \
         setTimeout(function () { done([log, text]); }, 50);",
    )
    .await;
    assert_eq!(
        non_blocking,
        json!([["wrote 14", "at 11", "wrote 1"], "Lorem ipsum!.."])
    );

    let paths = eval_callback(
        &t.session,
        "var fs = tizen.filesystem; var seen = []; \
         function fail(e) { done('error ' + e.name + ': ' + e.message); } \
         fs.createDirectory('documents/foo_dir', true, function (p) { seen.push(p); \
           fs.moveFile('documents/dir/log.txt', 'documents/foo_dir', false, function (p) { seen.push(p); \
             fs.rename('documents/foo_dir/log.txt', 'notes.txt', function (p) { seen.push(p); \
               fs.copyFile('documents/foo_dir/notes.txt', 'documents/foo_dir/copy.txt', false, function (p) { seen.push(p); \
                 fs.deleteFile('documents/foo_dir/copy.txt', function (p) { seen.push(p); \
                   fs.listDirectory('documents', function (names, path) { \
                     seen.push(names.sort().join(',') + ' in ' + path); \
                     fs.listDirectory('documents/foo_dir', function (names) { \
                       seen.push(names.join(',')); done(seen); \
                     }, fail, { name: 'NOTES%', isFile: true }); \
                   }, fail, { isDirectory: true }); \
                 }, fail); }, fail); }, fail); }, fail); }, fail);",
    )
    .await;
    assert_eq!(
        paths,
        json!([
            "documents/foo_dir",
            "documents/foo_dir/log.txt",
            "documents/foo_dir/notes.txt",
            "documents/foo_dir/copy.txt",
            "documents/foo_dir",
            "dir,foo_dir in documents",
            "notes.txt"
        ])
    );

    let checks = eval_callback(
        &t.session,
        "var fs = tizen.filesystem; var missing; \
         try { fs.isFile('documents/nope'); } catch (e) { missing = e.name; } \
         done([fs.isFile('documents/foo_dir/notes.txt'), fs.isDirectory('documents/foo_dir'), \
           fs.pathExists('documents/nope'), missing, fs.getDirName('documents/foo_dir/notes.txt/'), \
           fs.maxNameLength, fs.maxPathLength]);",
    )
    .await;
    assert_eq!(
        checks,
        json!([
            true,
            true,
            false,
            "NotFoundError",
            "documents/foo_dir",
            255,
            4096
        ])
    );

    // toURI gives a URL the page can load the file from.
    let loaded = eval_callback(
        &t.session,
        "var x = new XMLHttpRequest(); \
         x.open('GET', tizen.filesystem.toURI('documents/foo_dir/notes.txt')); \
         x.onload = function () { done(x.responseText); }; x.send();",
    )
    .await;
    assert_eq!(loaded, "Lorem ipsum!..");
    // The App's files aren't open to anyone else on the machine.
    let uri = t
        .session
        .evaluate("tizen.filesystem.toURI('documents/foo_dir/notes.txt')")
        .await
        .unwrap();
    let tokenless = uri.as_str().unwrap().split('?').next().unwrap().to_string();
    let status = reqwest::get(&tokenless).await.unwrap().status();
    assert_eq!(status, reqwest::StatusCode::FORBIDDEN, "{tokenless}");

    let console = console_texts(&mut events).await;
    assert!(
        !console
            .iter()
            .any(|text| text.contains("Synchronous XMLHttpRequest")),
        "the Bridge's requests showed in the App's console: {console:?}"
    );
    t.session.stop().await;
}

/// The pre-5.0 API (filesystem.html, 4.0 reference): File objects from
/// resolve(), FileStreams from openStream().
#[tokio::test]
async fn files_are_handled_through_file_objects_before_tizen_6() {
    if !has_file_api() {
        return;
    }
    let t = start("filesystem");
    t.session.wait_until_loaded().await.unwrap();
    let mut events = t.session.events();

    let file = eval_callback(
        &t.session,
        "function fail(e) { done('error ' + e.name + ': ' + e.message); } \
         tizen.filesystem.resolve('documents', function (docs) { \
           var dir = docs.createDirectory('music/ramones'); \
           var f = dir.createFile('song.txt'); \
           var exists; try { dir.createFile('song.txt'); } catch (e) { exists = e.name; } \
           f.openStream('w', function (s) { \
             s.write('h\\u00e9llo'); s.writeBytes([33, 33]); s.writeBase64(btoa('?')); \
             var at = s.position; s.close(); \
             var closed; try { s.write('x'); } catch (e) { closed = e instanceof Error; } \
             f.openStream('r', function (r) { \
               var info = [at, closed, exists, f.fileSize, r.bytesAvailable, r.read(2), r.position, \
                 r.readBytes(4), r.readBase64(10), r.eof, r.bytesAvailable]; \
               r.close(); \
               done(info.concat([f.path, f.name, f.fullPath, f.isFile, f.isDirectory, \
                 f.parent.fullPath, dir.length, docs.parent, docs.name, docs.path])); \
             }, fail); \
           }, fail); \
         }, fail, 'rw');",
    )
    .await;
    assert_eq!(
        file,
        json!([
            9,
            true,
            "IOError",
            9,
            9,
            "h\u{e9}",
            3,
            [108, 108, 111, 33],
            "IT8=",
            // A read that ends exactly at the end doesn't set eof (measured).
            false,
            0,
            "documents/music/ramones/",
            "song.txt",
            "documents/music/ramones/song.txt",
            true,
            false,
            "documents/music/ramones",
            1,
            null,
            "",
            "documents"
        ])
    );

    let changes = eval_callback(
        &t.session,
        "function fail(e) { done('error ' + e.name + ': ' + e.message); } \
         tizen.filesystem.resolve('documents/music', function (music) { \
           music.copyTo('documents/music/ramones/song.txt', 'documents/music/copy.txt', false, function () { \
             music.moveTo('documents/music/copy.txt', 'documents/music/moved.txt', false, function () { \
               music.listFiles(function (files) { \
                 var names = files.map(function (f) { return f.name; }).sort(); \
                 files.filter(function (f) { return f.isFile; })[0].readAsText(function (text) { \
                   music.deleteDirectory('documents/music/ramones', false, fail, function (e) { \
                     music.deleteDirectory('documents/music/ramones', true, function () { \
                       music.listFiles(function (left) { \
                         done([names, text, e.name, left.length]); \
                       }); \
                     }, fail); \
                   }); \
                 }, fail); \
               }, fail); \
             }, fail); \
           }, fail); \
         }, fail, 'rw');",
    )
    .await;
    assert_eq!(
        changes,
        json!([["moved.txt", "ramones"], "h\u{e9}llo!!?", "IOError", 1])
    );

    // wgt-package is read-only, so only mode r resolves it (#FileSystemManager::resolve).
    let errors = eval_callback(
        &t.session,
        "var fs = tizen.filesystem; var seen = []; \
         try { fs.resolve('documents', null); } catch (e) { seen.push(e.name); } \
         fs.resolve('wgt-package', function () { done('rw allowed'); }, function (e) { \
           seen.push(e.name); \
           fs.resolve('wgt-package/index.html', function (f) { \
             f.readAsText(function (text) { seen.push(text.indexOf('Filesystem fixture') >= 0); \
               fs.resolve('documents/nope', function () { done('found'); }, function (e) { \
                 seen.push(e.name); done(seen); \
               }); \
             }); \
           }, null, 'r'); \
         });",
    )
    .await;
    assert_eq!(
        errors,
        json!([
            "TypeMismatchError",
            "InvalidValuesError",
            true,
            "NotFoundError"
        ])
    );

    let console = console_texts(&mut events).await;
    assert!(
        !console
            .iter()
            .any(|text| text.contains("Synchronous XMLHttpRequest")),
        "the Bridge's requests showed in the App's console: {console:?}"
    );
    t.session.stop().await;
}

/// A TV lets an App reach paths outside the virtual roots ("file:///tmp",
/// filesystem.html); the Sandbox doesn't, so the call fails with a
/// Diagnostic naming the difference.
#[tokio::test]
async fn paths_outside_the_virtual_roots_fail_with_a_diagnostic() {
    let t = start("filesystem");
    t.session.wait_until_loaded().await.unwrap();
    let mut events = t.session.events();
    let (call, expected) = if has_path_api() {
        (
            "try { tizen.filesystem.pathExists('file:///tmp/x'); done('ok'); } \
             catch (e) { done(e.name); }",
            "InvalidValuesError",
        )
    } else {
        (
            "tizen.filesystem.resolve('file:///tmp', function () { done('ok'); }, \
               function (e) { done(e.name); }, 'r');",
            "NotFoundError",
        )
    };
    assert_eq!(eval_callback(&t.session, call).await, expected);
    expect_event(&mut events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "filesystem-outside-sandbox" && d.message.contains("file:///tmp"))
    })
    .await;
    t.session.stop().await;
}

/// Details of the 5.0 API the reference leaves open, as measured on a Tizen
/// 5.0 TV (UN50RU7100).
#[tokio::test]
async fn the_path_api_behaves_as_measured_on_a_tv() {
    if !has_path_api() {
        return;
    }
    let t = start("filesystem");
    t.session.wait_until_loaded().await.unwrap();
    let measured = eval_callback(
        &t.session,
        "var fs = tizen.filesystem; var out = []; \
         function name(f) { try { return f(); } catch (e) { return e.name; } } \
         out.push(fs.getDirName('abc'), fs.getDirName('/'), fs.getDirName('/a/b/'), fs.getDirName('a//b')); \
         out.push(fs.pathExists('wgt-private/../wgt-private')); \
         out.push(name(function () { return fs.openFile('foo/bar.txt', 'w'); })); \
         var w = fs.openFile('wgt-private/m/a.txt', 'w'); w.writeString('abc'); w.close(); \
         var d = fs.openFile('wgt-private/m', 'r'); \
         out.push(name(function () { return d.readString(); })); \
         var a = fs.openFile('wgt-private/m/a.txt', 'a'); out.push(a.seek(1), a.seek(0, 'CURRENT')); \
         a.writeString('Z'); a.close(); \
         out.push(fs.openFile('wgt-private/m/a.txt', 'r').readString()); \
         fs.listStorages(function (s) { out.push(s[0].label + ' ' + s.length); \
           fs.copyFile('wgt-private/m/a.txt', 'wgt-private/none/a.txt', false, null, function (e) { \
             out.push(e.name); \
             fs.listDirectory('wgt-private/m/a.txt', function () { done('listed'); }, function (e) { \
               out.push(e.name); done(out); }); }); });",
    )
    .await;
    assert_eq!(
        measured,
        json!([
            "abc",
            "/",
            "/a",
            "a",
            false,
            "NotFoundError",
            "IOError",
            1,
            1,
            "abcZ",
            "internal0 10",
            "IOError",
            "IOError"
        ])
    );
    t.session.stop().await;
}

/// Details of the pre-5.0 API the reference leaves open, as measured on a
/// Tizen 5.0 TV (UN50RU7100), which has both shapes.
#[tokio::test]
async fn file_objects_behave_as_measured_on_a_tv() {
    if !has_file_api() {
        return;
    }
    let t = start("filesystem");
    t.session.wait_until_loaded().await.unwrap();
    let measured = eval_callback(
        &t.session,
        "var fs = tizen.filesystem; var out = []; \
         function name(f) { try { return f(); } catch (e) { return e.name; } } \
         function fail(e) { done('error ' + e.name + ': ' + e.message); } \
         fs.resolve('documents', function (docs) { \
           docs.createDirectory('m'); \
           out.push(name(function () { return docs.createDirectory('m'); })); \
           out.push(name(function () { return docs.createFile('nodir/x.txt'); })); \
           var m = docs.resolve('m'); out.push(m.parent.fullPath, m.parent.path, m.path); \
           var f = m.createFile('e.txt'); \
           f.openStream('w', function (s) { s.write('XY'); s.close(); \
             var closed = name(function () { return s.read(1); }); \
             f.openStream('r', function (r) { \
               out.push(closed, r.eof, r.bytesAvailable, r.read(2), r.eof, r.bytesAvailable, \
                 r.read(1), r.eof, r.bytesAvailable); \
               r.close(); \
               fs.resolve('documents/m', function (ro) { \
                 out.push(name(function () { return ro.createFile('n.txt'); })); \
                 ro.copyTo('documents/m/e.txt', 'documents/m/c.txt', true, function () { \
                   out.push('copied'); \
                   ro.deleteFile('documents/m/c.txt', function () { done('deleted'); }, function (e) { \
                     out.push(e.name); \
                     ro.resolve('e.txt').openStream('w', function () { done('opened'); }, function (e) { \
                       out.push(e.name); done(out); }); }); \
                 }, fail); \
               }, fail, 'r'); \
             }, fail); }, fail); \
         }, fail, 'rw');",
    )
    .await;
    assert_eq!(
        measured,
        json!([
            "InvalidValuesError",
            "UnknownError",
            "documents/",
            "documents/",
            "documents/",
            "IOError",
            false,
            2,
            "XY",
            false,
            0,
            "",
            true,
            -1,
            "InvalidValuesError",
            "copied",
            "InvalidValuesError",
            "InvalidValuesError"
        ])
    );
    t.session.stop().await;
}
