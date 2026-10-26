//! GNOME Settings' Displays panel on Hyprland (ADR 0046, M8.51): kojed serves
//! Mutter's `org.gnome.Mutter.DisplayConfig` from Hyprland's screens. A
//! persistent change is written to monitors.xml and must be confirmed within
//! [`CONFIRM`] – the shell asks "Keep these display settings?" (M8.53) –
//! else kojed restores the screens and the file as they were: a mode the
//! screen cannot show never sticks. Decisions: `koje_core::mutter`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use koje_core::api::{INTERFACE, OBJECT_PATH};
use koje_core::monitors;
use koje_core::mutter::{self, LogicalView, MonitorView, RequestedLogical};
use koje_core::paths::Paths;
use koje_hyprland::{Hyprland, requests};
use tokio::sync::Mutex;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

pub const NAME: &str = "org.gnome.Mutter.DisplayConfig";
pub const PATH: &str = "/org/gnome/Mutter/DisplayConfig";
/// How long a persistent change waits to be kept (GNOME Shell's 20 s).
pub const CONFIRM: Duration = Duration::from_secs(20);
const DRM: &str = "/sys/class/drm";
const TIMEOUT: Duration = Duration::from_secs(5);

/// A change waiting to be kept.
#[derive(Debug)]
struct Pending {
    previous_lines: Vec<String>,
    /// monitors.xml before the change; `None`: there was none.
    previous_xml: Option<String>,
    revert: tokio::task::AbortHandle,
}

#[derive(Debug, Default)]
struct State {
    serial: u32,
    pending: Option<Pending>,
}

/// The screens' state shared by Mutter's API, kojed's own API and the
/// hotplug watcher.
#[derive(Debug)]
pub struct Displays {
    hypr: Hyprland,
    paths: Paths,
    runtime_conf: PathBuf,
    state: Mutex<State>,
    conn: OnceLock<zbus::Connection>,
}

type MonitorTuple = (
    (String, String, String, String),
    Vec<(
        String,
        i32,
        i32,
        f64,
        f64,
        Vec<f64>,
        HashMap<String, OwnedValue>,
    )>,
    HashMap<String, OwnedValue>,
);
type LogicalTuple = (
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<(String, String, String, String)>,
    HashMap<String, OwnedValue>,
);
type RequestTuple = (
    i32,
    i32,
    f64,
    u32,
    bool,
    Vec<(String, String, HashMap<String, OwnedValue>)>,
);

fn owned(v: &Value<'_>) -> Option<OwnedValue> {
    v.try_to_owned().ok()
}

fn props(entries: Vec<(&str, Value<'_>)>) -> HashMap<String, OwnedValue> {
    entries
        .into_iter()
        .filter_map(|(k, v)| owned(&v).map(|v| (k.to_owned(), v)))
        .collect()
}

fn i32_of(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

impl Displays {
    pub fn new(hypr: Hyprland, paths: Paths, runtime: &Path) -> Arc<Self> {
        Arc::new(Self {
            hypr,
            paths,
            runtime_conf: runtime.join("koje/monitors.conf"),
            state: Mutex::new(State::default()),
            conn: OnceLock::new(),
        })
    }

    /// Serve Mutter's API on `conn` – unless a Mutter owns its name (kojed
    /// in a GNOME session) – and use `conn` for signals from timers and the
    /// hotplug watcher.
    pub async fn serve(self: &Arc<Self>, conn: &zbus::Connection) -> zbus::Result<()> {
        let _ = self.conn.set(conn.clone());
        conn.object_server()
            .at(
                PATH,
                DisplayConfig {
                    displays: Arc::clone(self),
                },
            )
            .await?;
        match conn.request_name(NAME).await {
            Ok(()) => tracing::info!("serving {NAME}"),
            Err(e) => tracing::warn!("not serving {NAME}: {e}"),
        }
        Ok(())
    }

    async fn read(&self) -> anyhow::Result<(Vec<MonitorView>, Vec<LogicalView>)> {
        let json = tokio::time::timeout(TIMEOUT, self.hypr.request("j/monitors all")).await??;
        let hypr = mutter::parse_monitors(&json).map_err(anyhow::Error::msg)?;
        let ids: BTreeMap<_, _> = monitors::read_connected(Path::new(DRM))
            .into_iter()
            .map(|s| (s.connector, s.id))
            .collect();
        Ok(mutter::current_state(&hypr, &ids))
    }

    async fn apply_lines(&self, lines: &[String]) -> anyhow::Result<()> {
        if let Err(e) = koje_core::fsutil::write_atomic(
            &self.runtime_conf,
            monitors::hyprland_conf(lines).as_bytes(),
        ) {
            tracing::warn!("writing {}: {e}", self.runtime_conf.display());
        }
        let keywords: Vec<String> = lines
            .iter()
            .map(|l| format!("keyword monitor {l}"))
            .collect();
        let request = requests::batch(&keywords);
        let reply = tokio::time::timeout(TIMEOUT, self.hypr.request(&request)).await??;
        requests::check_reply(&request, &reply)?;
        Ok(())
    }

    /// The screens changed (applied, reverted, plugged): a new serial, and
    /// GNOME Settings reads them again.
    pub async fn changed(&self) {
        self.state.lock().await.serial += 1;
        if let Some(conn) = self.conn.get()
            && let Ok(e) = SignalEmitter::new(conn, PATH)
        {
            let _ = DisplayConfig::monitors_changed(&e).await;
        }
    }

    /// Keep a pending change (the shell's "Keep Changes").
    pub async fn keep(&self) -> bool {
        let pending = self.state.lock().await.pending.take();
        if let Some(p) = &pending {
            p.revert.abort();
            tracing::info!("display change kept");
        }
        pending.is_some()
    }

    /// Undo a pending change now (the shell's "Revert").
    pub async fn revert(self: &Arc<Self>) -> bool {
        let Some(p) = self.state.lock().await.pending.take() else {
            return false;
        };
        p.revert.abort();
        self.undo(p).await;
        true
    }

    /// The timer ran out: undo. Not through [`Self::revert`], whose abort
    /// would cancel this very task in the middle of it.
    async fn expire(self: &Arc<Self>) {
        let pending = self.state.lock().await.pending.take();
        if let Some(p) = pending {
            tracing::info!("display change not kept in time");
            self.undo(p).await;
        }
    }

    async fn undo(self: &Arc<Self>, p: Pending) {
        let xml = self.paths.monitors_xml();
        let restored = match &p.previous_xml {
            Some(text) => {
                koje_core::fsutil::write_atomic(&xml, text.as_bytes()).map_err(|e| e.to_string())
            }
            None => std::fs::remove_file(&xml).map_err(|e| e.to_string()),
        };
        if let Err(e) = restored {
            tracing::warn!("restoring {}: {e}", xml.display());
        }
        if let Err(e) = self.apply_lines(&p.previous_lines).await {
            tracing::warn!("reverting the screens: {e:#}");
        }
        tracing::info!("display change reverted");
        self.changed().await;
    }

    async fn apply(
        self: &Arc<Self>,
        serial: u32,
        method: u32,
        request: Vec<RequestTuple>,
    ) -> fdo::Result<()> {
        if serial != self.state.lock().await.serial {
            return Err(fdo::Error::AccessDenied(
                "The requested configuration is based on stale information".into(),
            ));
        }
        let (monitors_now, logical_now) = self
            .read()
            .await
            .map_err(|e| fdo::Error::Failed(format!("{e:#}")))?;
        let request: Vec<RequestedLogical> = request
            .into_iter()
            .map(
                |(x, y, scale, transform, primary, monitors)| RequestedLogical {
                    x,
                    y,
                    scale,
                    transform,
                    primary,
                    monitors: monitors.into_iter().map(|(c, m, _)| (c, m)).collect(),
                },
            )
            .collect();
        let (lines, conf) =
            mutter::apply_request(&request, &monitors_now).map_err(fdo::Error::InvalidArgs)?;
        // 0: only check.
        if method == 0 {
            return Ok(());
        }
        // A change on top of an unconfirmed one goes back to the state
        // before both.
        let earlier = self.state.lock().await.pending.take();
        if let Some(p) = &earlier {
            p.revert.abort();
        }
        let previous_lines = earlier.as_ref().map_or_else(
            || mutter::current_lines(&monitors_now, &logical_now),
            |p| p.previous_lines.clone(),
        );
        self.apply_lines(&lines)
            .await
            .map_err(|e| fdo::Error::Failed(format!("{e:#}")))?;
        // 2: persistent – monitors.xml, kept only when confirmed.
        if method == 2 {
            let path = self.paths.monitors_xml();
            let previous_xml = match earlier {
                Some(p) => p.previous_xml,
                None => std::fs::read_to_string(&path).ok(),
            };
            let mut xml = previous_xml
                .as_deref()
                .and_then(|t| monitors::parse(t).ok())
                .unwrap_or(monitors::MonitorsXml {
                    configurations: Vec::new(),
                    extra: Vec::new(),
                });
            monitors::put(&mut xml, conf);
            koje_core::fsutil::write_atomic(&path, monitors::to_xml(&xml).as_bytes())
                .map_err(|e| fdo::Error::Failed(e.to_string()))?;
            let me = Arc::clone(self);
            let timer = tokio::spawn(async move {
                tokio::time::sleep(CONFIRM).await;
                me.expire().await;
            });
            self.state.lock().await.pending = Some(Pending {
                previous_lines,
                previous_xml,
                revert: timer.abort_handle(),
            });
            self.ask_to_keep().await;
        }
        tracing::info!(
            method,
            screens = lines.len(),
            "applied a display configuration"
        );
        self.changed().await;
        Ok(())
    }

    /// Ask the shell to show "Keep these display settings?" (M8.53).
    async fn ask_to_keep(&self) {
        if let Some(conn) = self.conn.get() {
            let seconds = u32::try_from(CONFIRM.as_secs()).unwrap_or(20);
            if let Err(e) = conn
                .emit_signal(
                    None::<&str>,
                    OBJECT_PATH,
                    INTERFACE,
                    "DisplayChangePending",
                    &(seconds,),
                )
                .await
            {
                tracing::debug!("DisplayChangePending: {e}");
            }
        }
    }
}

/// `org.gnome.Mutter.DisplayConfig`, as GNOME Settings uses it.
#[derive(Debug)]
pub struct DisplayConfig {
    pub displays: Arc<Displays>,
}

#[zbus::interface(name = "org.gnome.Mutter.DisplayConfig")]
#[allow(clippy::unused_self, reason = "zbus properties are methods")]
impl DisplayConfig {
    #[zbus(out_args("serial", "monitors", "logical_monitors", "properties"))]
    async fn get_current_state(
        &self,
    ) -> fdo::Result<(
        u32,
        Vec<MonitorTuple>,
        Vec<LogicalTuple>,
        HashMap<String, OwnedValue>,
    )> {
        let (monitors, logical) = self
            .displays
            .read()
            .await
            .map_err(|e| fdo::Error::Failed(format!("{e:#}")))?;
        let serial = self.displays.state.lock().await.serial;
        let spec = |s: &koje_core::monitors::Spec| {
            (
                s.connector.clone(),
                s.id.vendor.clone(),
                s.id.product.clone(),
                s.id.serial.clone(),
            )
        };
        let monitors = monitors
            .iter()
            .map(|m| {
                let modes = m
                    .modes
                    .iter()
                    .map(|v| {
                        let mut p = vec![];
                        if v.current {
                            p.push(("is-current", Value::from(true)));
                        }
                        if v.preferred {
                            p.push(("is-preferred", Value::from(true)));
                        }
                        (
                            v.id.clone(),
                            i32_of(v.width),
                            i32_of(v.height),
                            v.rate,
                            v.preferred_scale,
                            v.scales.clone(),
                            props(p),
                        )
                    })
                    .collect();
                let mut p = vec![("display-name", Value::from(m.display_name.clone()))];
                if m.builtin {
                    p.push(("is-builtin", Value::from(true)));
                }
                if m.width_mm > 0 {
                    p.push(("width-mm", Value::from(i32_of(m.width_mm))));
                    p.push(("height-mm", Value::from(i32_of(m.height_mm))));
                }
                (spec(&m.spec), modes, props(p))
            })
            .collect();
        let logical = logical
            .iter()
            .map(|l| {
                (
                    l.x,
                    l.y,
                    l.scale,
                    l.transform,
                    l.primary,
                    l.monitors.iter().map(spec).collect(),
                    HashMap::new(),
                )
            })
            .collect();
        let properties = props(vec![
            // 1: logical – positions in scaled pixels, as Hyprland's.
            ("layout-mode", Value::from(1u32)),
            ("supports-changing-layout-mode", Value::from(false)),
            ("global-scale-required", Value::from(false)),
        ]);
        Ok((serial, monitors, logical, properties))
    }

    async fn apply_monitors_config(
        &self,
        serial: u32,
        method: u32,
        logical_monitors: Vec<RequestTuple>,
        properties: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        // Only the logical layout (scaled pixels, Hyprland's) is offered.
        if let Some(mode) = properties.get("layout-mode")
            && u32::try_from(mode).ok() != Some(1)
        {
            return Err(fdo::Error::InvalidArgs(
                "only the logical layout mode is supported".into(),
            ));
        }
        self.displays.apply(serial, method, logical_monitors).await
    }

    #[zbus(signal)]
    pub async fn monitors_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// 0: on; 1–3: standby, suspend, off – all as Hyprland's DPMS off.
    #[zbus(property)]
    fn power_save_mode(&self) -> i32 {
        0
    }

    #[zbus(property)]
    async fn set_power_save_mode(&self, mode: i32) -> zbus::Result<()> {
        let request = if mode == 0 {
            "dispatch dpms on"
        } else {
            "dispatch dpms off"
        };
        self.displays
            .hypr
            .request(request)
            .await
            .map(drop)
            .map_err(|e| zbus::Error::Failure(e.to_string()))
    }

    #[zbus(property)]
    fn panel_orientation_managed(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn apply_monitors_config_allowed(&self) -> bool {
        true
    }

    /// Night light is hyprsunset's (M8.44), not gamma through this API.
    #[zbus(property)]
    fn night_light_supported(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_external_monitor(&self) -> bool {
        monitors::read_connected(Path::new(DRM))
            .iter()
            .any(|s| !mutter::is_builtin(&s.connector))
    }
}
