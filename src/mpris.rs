//! TUSIC MPRIS — minimal synchronous D-Bus MediaPlayer2 server.
//! Lets media keys / playerctl control playback. Uses the `dbus-crossroads`
//! crate (pure-Rust, tiny, no async reactor) — lean for the i3.

use crate::app::App;
use dbus::arg::messageitem::{MessageItem, MessageItemArray};
use dbus::arg::{RefArg, Variant};
use dbus::blocking::Connection;
use dbus::channel::Sender as DbSender;
use dbus_crossroads::Crossroads;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Mpris {
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Mpris {
    /// Spawn the D-Bus server. Shares `app_ref` with the TUI loop so media
    /// keys update the same state the UI renders.
    pub fn spawn(app_ref: Arc<Mutex<App>>) -> Mpris {
        let handle = std::thread::spawn(move || run_server(app_ref));
        Mpris { handle: Some(handle) }
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn safe_name() -> String {
    // D-Bus bus names forbid hyphens: HUMAN-GITA → HUMANGITA
    crate::BRAND
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

type MetaMap = HashMap<String, Variant<Box<dyn RefArg>>>;

/// MPRIS spec status casing ("Playing"/"Paused"/"Stopped"). The TUI state
/// word is uppercase ("PLAYING") — shells/playerctl match the spec case, so
/// uppercase here silently breaks media-key routing.
fn spec_status(word: &str) -> &'static str {
    match word {
        "PLAYING" => "Playing",
        "PAUSED" => "Paused",
        _ => "Stopped",
    }
}

/// Minimal `dbus::channel::Sender` shim for async_support.
struct ConnProxy;
impl DbSender for ConnProxy {
    fn send(&self, _msg: dbus::Message) -> Result<u32, ()> {
        Err(())
    }
}

/// Broadcast PropertiesChanged every 500ms while playing (and on every state
/// or track change). Payload: PlaybackStatus + Position always, Metadata on
/// track change. Signature sa{sv}as — interface, dict, invalidated props.
fn run_emitter(app_ref: Arc<Mutex<App>>) {
    let conn = match Connection::new_session() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut last_state = String::new();
    let mut last_title = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let (status, title, pos_us) = {
            let app = match app_ref.lock() {
                Ok(a) => a,
                Err(_) => continue,
            };
            (
                spec_status(app.state_word()).to_string(),
                app.current
                    .as_ref()
                    .map(|t| t.title.clone())
                    .unwrap_or_default(),
                (app.display_pos * 1_000_000.0) as i64,
            )
        };
        let track_changed = title != last_title;
        if !track_changed && status == last_state && status != "Playing" {
            last_state = status;
            last_title = title;
            continue;
        }
        let mut dict_items: Vec<(MessageItem, MessageItem)> = vec![
            (
                "PlaybackStatus".into(),
                MessageItem::Variant(Box::new(MessageItem::Str(status.clone()))),
            ),
            (
                "Position".into(),
                MessageItem::Variant(Box::new(MessageItem::Int64(pos_us))),
            ),
        ];
        if track_changed {
            let mut m: Vec<(MessageItem, MessageItem)> = Vec::new();
            if let Ok(a) = app_ref.lock() {
                if let Some(t) = &a.current {
                    m.push((
                        "xesam:title".into(),
                        MessageItem::Variant(Box::new(MessageItem::Str(t.title.clone()))),
                    ));
                    m.push((
                        "xesam:artist".into(),
                        MessageItem::Variant(Box::new(MessageItem::new_array(vec![
                            MessageItem::Str(t.artist.clone()),
                        ])
                        .unwrap())),
                    ));
                    if !t.album.is_empty() {
                        m.push((
                            "xesam:album".into(),
                            MessageItem::Variant(Box::new(MessageItem::Str(t.album.clone()))),
                        ));
                    }
                    m.push((
                        "mpris:length".into(),
                        MessageItem::Variant(Box::new(MessageItem::Int64(
                            (t.duration * 1_000_000.0) as i64,
                        ))),
                    ));
                }
            }
            // Metadata: only when a track is loaded. new_dict() on an empty
            // vec errors with EmptyArray (dbus-rs) — that unwrap panicked the
            // emitter thread at startup (no current track yet).
            if !m.is_empty() {
                dict_items.push((
                    "Metadata".into(),
                    MessageItem::Variant(Box::new(MessageItem::new_dict(m).unwrap())),
                ));
            }
        }
        let mut msg = dbus::Message::signal(
            &dbus::strings::Path::new("/org/mpris/MediaPlayer2").unwrap(),
            &dbus::strings::Interface::new("org.freedesktop.DBus.Properties").unwrap(),
            &dbus::strings::Member::new("PropertiesChanged").unwrap(),
        );
        msg.append_items(&[
            "org.mpris.MediaPlayer2.Player".to_string().into(),
            MessageItem::new_dict(dict_items).unwrap(),
            MessageItem::Array(
                MessageItemArray::new(
                    Vec::new(),
                    dbus::strings::Signature::new("as").unwrap(),
                )
                .unwrap(),
            ),
        ]);
        let _ = conn.channel().send(msg);
        last_state = status;
        last_title = title;
    }
}

fn run_server(app_ref: Arc<Mutex<App>>) {
    // Signal emitter: broadcasts PropertiesChanged on its own session
    // connection so desktop shells (COSMIC/GNOME media OSD) learn this
    // player is active and route media keys here. mpv registers its own
    // MPRIS name on the same bus; the shell picks the player that SIGNALS —
    // without these, kalki is invisible to media keys.
    std::thread::spawn({
        let a = app_ref.clone();
        move || run_emitter(a)
    });

    let conn = match Connection::new_session() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[mpris] no session bus: {e}");
            return;
        }
    };
    let name = format!("org.mpris.MediaPlayer2.{}", safe_name());
    if conn.request_name(&name, false, true, false).is_err() {
        eprintln!("[mpris] MPRIS name taken: {name}");
        return;
    }

    let mut cr = Crossroads::new();
    cr.set_async_support(Some((
        Arc::new(ConnProxy) as Arc<dyn DbSender + Send + Sync>,
        Box::new(|f| {
            std::thread::spawn(|| f);
            ()
        }),
    )));

    // ---- org.mpris.MediaPlayer2 (root) ----
    let root_token = cr.register("org.mpris.MediaPlayer2", |b| {
        b.property("Identity")
            .get(|_, _: &mut ()| Ok(crate::BRAND.to_string()));
        b.property("CanQuit").get(|_, _: &mut ()| Ok(false));
        b.property("CanRaise").get(|_, _: &mut ()| Ok(false));
        b.property("HasTrackList").get(|_, _: &mut ()| Ok(false));
        b.property("SupportedUriSchemes")
            .get(|_, _: &mut ()| Ok(vec!["http".to_string(), "https".to_string()]));
        b.property("SupportedMimeTypes")
            .get(|_, _: &mut ()| Ok(Vec::<String>::new()));
        b.method("Raise", (), (), |_, _, _: ()| Ok(()));
        b.method("Quit", (), (), |_, _, _: ()| Ok(()));
    });

    // ---- org.mpris.MediaPlayer2.Player ----
    let pp = app_ref.clone();
    let pl = app_ref.clone();
    let pa = app_ref.clone();
    let pn = app_ref.clone();
    let pr = app_ref.clone();
    let ps = app_ref.clone();
    let psk = app_ref.clone();
    let psetpos = app_ref.clone();
    let psetvol = app_ref.clone();
    let pstatus = app_ref.clone();
    let pmeta = app_ref.clone();
    let ppos = app_ref.clone();
    let pvol = app_ref.clone();

    let player_token = cr.register("org.mpris.MediaPlayer2.Player", move |b| {
        b.method("PlayPause", (), (), {
            let a = pp.clone();
            move |_, _, _: ()| {
                if let Ok(mut app) = a.lock() {
                    let p = app.paused;
                    app.set_pause(!p);
                }
                Ok(())
            }
        });
        b.method("Play", (), (), {
            let a = pl.clone();
            move |_, _, _: ()| {
                if let Ok(mut app) = a.lock() {
                    app.set_pause(false);
                }
                Ok(())
            }
        });
        b.method("Pause", (), (), {
            let a = pa.clone();
            move |_, _, _: ()| {
                if let Ok(mut app) = a.lock() {
                    app.set_pause(true);
                }
                Ok(())
            }
        });
        b.method("Next", (), (), {
            let a = pn.clone();
            move |_, _, _: ()| {
                if let Ok(mut app) = a.lock() {
                    app.next_track(true);
                }
                Ok(())
            }
        });
        b.method("Previous", (), (), {
            let a = pr.clone();
            move |_, _, _: ()| {
                if let Ok(mut app) = a.lock() {
                    app.prev_track();
                }
                Ok(())
            }
        });
        b.method("Stop", (), (), {
            let a = ps.clone();
            move |_, _, _: ()| {
                if let Ok(mut app) = a.lock() {
                    app.stop_after_public();
                }
                Ok(())
            }
        });
        b.method("Seek", ("x",), (), {
            let a = psk.clone();
            move |_, _, (offset,): (i64,)| {
                if let Ok(mut app) = a.lock() {
                    app.seek_rel(offset as f64 / 1_000_000.0);
                }
                Ok(())
            }
        });
        b.method("SetPosition", ("o", "x"), (), {
            let a = psetpos.clone();
            move |_, _, (_track, pos): (dbus::Path<'static>, i64)| {
                if let Ok(mut app) = a.lock() {
                    app.seek_abs_public(pos as f64 / 1_000_000.0);
                }
                Ok(())
            }
        });
        b.method("SetVolume", ("d",), (), {
            let a = psetvol.clone();
            move |_, _, (vol,): (f64,)| {
                if let Ok(mut app) = a.lock() {
                    app.set_volume_public((vol * 100.0) as i64);
                }
                Ok(())
            }
        });

        b.property("PlaybackStatus")
            .get(move |_, _: &mut ()| {
                let s = pstatus
                    .lock()
                    .map(|a| a.state_word().to_string())
                    .unwrap_or_else(|_| "Stopped".into());
                Ok(spec_status(&s).to_string())
            });
        b.property("Metadata").get(move |_, _: &mut ()| {
            let mut m: MetaMap = HashMap::new();
            if let Ok(a) = pmeta.lock() {
                if let Some(t) = &a.current {
                    m.insert("xesam:title".into(), Variant(Box::new(t.title.clone())));
                    m.insert(
                        "xesam:artist".into(),
                        Variant(Box::new(vec![t.artist.clone()])),
                    );
                    if !t.album.is_empty() {
                        m.insert("xesam:album".into(), Variant(Box::new(t.album.clone())));
                    }
                    m.insert(
                        "mpris:length".into(),
                        Variant(Box::new((t.duration * 1_000_000.0) as i64)),
                    );
                }
            }
            Ok(m)
        });
        b.property("Position").get(move |_, _: &mut ()| {
            let p = ppos
                .lock()
                .map(|a| (a.display_pos * 1_000_000.0) as i64)
                .unwrap_or(0);
            Ok(p)
        });
        b.property("Volume").get(move |_, _: &mut ()| {
            let v = pvol.lock().map(|a| a.volume as f64 / 100.0).unwrap_or(0.7);
            Ok(v)
        });
        b.property("CanPlay").get(|_, _: &mut ()| Ok(true));
        b.property("CanPause").get(|_, _: &mut ()| Ok(true));
        b.property("CanSeek").get(|_, _: &mut ()| Ok(true));
        b.property("CanControl").get(|_, _: &mut ()| Ok(true));
        b.property("CanGoNext").get(|_, _: &mut ()| Ok(true));
        b.property("CanGoPrevious").get(|_, _: &mut ()| Ok(true));
        b.property("MinimumRate").get(|_, _: &mut ()| Ok(1.0f64));
        b.property("MaximumRate").get(|_, _: &mut ()| Ok(1.0f64));
    });

    cr.insert("/org/mpris/MediaPlayer2", &[root_token, player_token], ());
    if let Err(e) = cr.serve(&conn) {
        eprintln!("[mpris] serve failed: {e}");
    }
}
