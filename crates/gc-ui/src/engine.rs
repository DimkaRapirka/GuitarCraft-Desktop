//! WebView endpoint for the native audio engine. Only device I/O and DSP cross Tauri IPC.
use crate::{
    dom::{get, invoke, num, set, string},
    host::{command, now, resolved, sleep},
    js, read,
};
use js_sys::{Array, Object, Uint8Array};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::spawn_local;

struct State {
    project: JsValue,
    config: RefCell<Value>,
    status: RefCell<JsValue>,
    context: JsValue,
    audition: RefCell<JsValue>,
    meters: RefCell<JsValue>,
    gate: JsValue,
    spectrum: RefCell<JsValue>,
    spectrum_request: Cell<f64>,
    live_info: RefCell<JsValue>,
    di_name: RefCell<String>,
    playing: Cell<bool>,
    bpm: Cell<f64>,
    metronome: Cell<bool>,
    muted: Cell<bool>,
    polling: Cell<bool>,
    scheduled: Cell<bool>,
    pending: RefCell<Option<JsValue>>,
}
fn flag(name: &'static str, key: &str, value: Value) {
    let args = json!({key:value});
    fire(name, args);
}
fn fire(name: &'static str, args: Value) {
    spawn_local(async move {
        if let Err(e) = cmd(name, &args).await {
            web_sys::console::warn_2(&name.into(), &e);
        }
    });
}
async fn cmd(name: &str, args: &Value) -> Result<JsValue, JsValue> {
    command(name, &js(args)?).await
}
impl State {
    async fn refresh(&self) -> Result<JsValue, JsValue> {
        let status = command("engine_status", &JsValue::UNDEFINED).await?;
        set(&self.context, "sampleRate", &get(&status, "sr"));
        set(
            &self.context,
            "baseLatency",
            &(num(&status, "base_ms", 0.) / 1000.).into(),
        );
        set(
            &self.context,
            "outputLatency",
            &(num(&status, "out_ms", 0.) / 1000.).into(),
        );
        let live = if get(&status, "live").is_truthy() {
            js(
                &json!({"deviceId":string(&status,"input"),"label":string(&status,"input"),"channel":num(&status,"channel",0.),"channels":num(&status,"inChannels",1.)}),
            )?
        } else {
            JsValue::NULL
        };
        self.live_info.replace(live);
        let before = self.status.replace(status.clone());
        let error = string(&status, "outputError");
        if num(&status, "sr", 0.) > 0. && !error.is_empty() && error != string(&before, "outputError") {
            crate::desktop::toast_error(&crate::i18n::message("Аудио: {}", &[crate::i18n::text(&error)]).into());
        }
        Ok(status)
    }
    async fn configure(&self, patch: Value) -> Result<JsValue, JsValue> {
        let cfg = {
            let mut c = self.config.borrow_mut();
            if let Some(p) = patch.as_object() {
                c.as_object_mut().unwrap().extend(p.clone());
            }
            c.clone()
        };
        let device = |k: &str| cfg[k].as_str().filter(|s| !s.is_empty() && *s != "default");
        let n = |k: &str, default: i64| {
            cfg[k]
                .as_i64()
                .or_else(|| cfg[k].as_str().and_then(|v| v.parse().ok()))
                .unwrap_or(default)
        };
        let info=cmd("engine_configure",&json!({"cfg":{"mode":cfg["mode"],"input":device("input"),"output":device("output"),"channel":n("channel",0),
            "output_pair":n("outputPair",0),"mono":cfg["mono"]==true,"buffer":n("buffer",128),"sample_rate":Some(n("sampleRate",0)).filter(|n|*n>0)}})).await?;
        self.refresh().await?;
        Ok(info)
    }
    fn poll(self: &Rc<Self>) {
        if self.polling.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        spawn_local(async move {
            while let Some(state) = weak.upgrade() {
                if let Ok(m) = command("engine_meters", &JsValue::UNDEFINED).await {
                    set(&get(&state.gate, "status"), "open", &get(&m, "gateOpen"));
                    state.meters.replace(m);
                    if now() - state.spectrum_request.get() < 600. {
                        if let Ok(spec) = cmd("engine_spectrum", &json!({"n":8192})).await {
                            state.spectrum.replace(spec);
                        }
                    }
                }
                drop(state);
                if sleep(33.).await.is_err() {
                    break;
                }
            }
        });
        let weak = Rc::downgrade(self);
        spawn_local(async move {
            while sleep(1000.).await.is_ok() {
                let Some(s) = weak.upgrade() else { break };
                let _ = s.refresh().await;
            }
        });
    }
    fn queue(self: &Rc<Self>, state: JsValue) {
        self.pending.replace(Some(state));
        if self.scheduled.replace(true) {
            return;
        }
        let s = self.clone();
        let cb = Closure::once_into_js(move |_: f64| {
            spawn_local(async move {
                loop {
                    let state = s.pending.borrow_mut().take();
                    let Some(state) = state else {
                        break;
                    };
                    let rig = invoke(
                        &s.project,
                        "audio_rig",
                        &[state, s.audition.borrow().clone(), s.bpm.get().into()],
                    );
                    match rig {
                        Ok(rig) => {
                            let args = Object::new();
                            set(&args, "rig", &rig);
                            if let Err(e) = command("engine_set_rig", &args).await {
                                web_sys::console::warn_1(&e);
                            }
                        }
                        Err(e) => web_sys::console::warn_1(&e),
                    }
                }
                s.scheduled.set(false);
            });
        });
        if web_sys::window()
            .unwrap()
            .request_animation_frame(cb.unchecked_ref())
            .is_err()
        {
            self.scheduled.set(false);
        }
    }
}
#[wasm_bindgen]
pub struct RustEngine {
    state: Rc<State>,
}
#[wasm_bindgen]
impl RustEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(project: JsValue) -> Result<RustEngine, JsValue> {
        Ok(Self {
            state: Rc::new(State {
                project,
                config: RefCell::new(
                    json!({"mode":"di","input":null,"channel":0,"output":null,"buffer":128,"sampleRate":null}),
                ),
                status: RefCell::new(Object::new().into()),
                context: js(
                    &json!({"sampleRate":48000,"baseLatency":0.003,"outputLatency":0,"state":"running"}),
                )?,
                audition: RefCell::new(JsValue::NULL),
                meters: RefCell::new(js(&json!({"raw":0,"input":0,"output":0,"gateOpen":false}))?),
                gate: js(&json!({"status":{"open":false}}))?,
                spectrum: RefCell::new(js_sys::Float32Array::from(&[-120f32; 4097][..]).into()),
                spectrum_request: Cell::new(f64::NEG_INFINITY),
                live_info: RefCell::new(JsValue::NULL),
                di_name: Default::default(),
                playing: Default::default(),
                bpm: Cell::new(120.),
                metronome: Default::default(),
                muted: Default::default(),
                polling: Default::default(),
                scheduled: Default::default(),
                pending: Default::default(),
            }),
        })
    }
    pub async fn init(&self, cfg: JsValue) -> Result<(), JsValue> {
        let result = self.configure(cfg).await;
        self.state.poll();
        result.map(|_| ())
    }
    pub async fn configure(&self, cfg: JsValue) -> Result<JsValue, JsValue> {
        self.state
            .configure(if cfg.is_object() {
                read(cfg)?
            } else {
                json!({})
            })
            .await
    }
    #[wasm_bindgen(js_name=refreshStatus)]
    pub async fn refresh_status(&self) -> Result<JsValue, JsValue> {
        self.state.refresh().await
    }
    #[wasm_bindgen(getter)]
    pub fn ctx(&self) -> JsValue {
        self.state.context.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> JsValue {
        self.state.status.borrow().clone()
    }
    #[wasm_bindgen(getter)]
    pub fn gate(&self) -> JsValue {
        self.state.gate.clone()
    }
    #[wasm_bindgen(getter,js_name=inAnalyser)]
    pub fn in_analyser(&self) -> String {
        "input".into()
    }
    #[wasm_bindgen(getter,js_name=outAnalyser)]
    pub fn out_analyser(&self) -> String {
        "output".into()
    }
    #[wasm_bindgen(getter,js_name=rawAnalyser)]
    pub fn raw_analyser(&self) -> String {
        "raw".into()
    }
    #[wasm_bindgen(getter,js_name=tunerAnalyser)]
    pub fn tuner_analyser(&self) -> String {
        "raw".into()
    }
    #[wasm_bindgen(getter,js_name=liveInfo)]
    pub fn live_info(&self) -> JsValue {
        self.state.live_info.borrow().clone()
    }
    #[wasm_bindgen(getter,js_name=liveNode)]
    pub fn live_node(&self) -> JsValue {
        self.live_info()
    }
    #[wasm_bindgen(getter,js_name=diName)]
    pub fn di_name(&self) -> String {
        self.state.di_name.borrow().clone()
    }
    #[wasm_bindgen(getter,js_name=diPlaying)]
    pub fn di_playing(&self) -> bool {
        self.state.playing.get()
    }
    #[wasm_bindgen(getter)]
    pub fn metronome(&self) -> bool {
        self.state.metronome.get()
    }
    #[wasm_bindgen(getter)]
    pub fn muted(&self) -> bool {
        self.state.muted.get()
    }
    #[wasm_bindgen(getter)]
    pub fn bpm(&self) -> f64 {
        self.state.bpm.get()
    }
    #[wasm_bindgen(setter)]
    pub fn set_bpm(&self, bpm: f64) {
        if bpm.is_finite() && bpm > 0. {
            self.state.bpm.set(bpm);
        }
    }
    #[wasm_bindgen(getter)]
    pub fn audition(&self) -> JsValue {
        self.state.audition.borrow().clone()
    }
    #[wasm_bindgen(setter)]
    pub fn set_audition(&self, a: JsValue) {
        self.state.audition.replace(a);
    }
    #[wasm_bindgen(getter,js_name=latencyMs)]
    pub fn latency_ms(&self) -> f64 {
        1000.
            * (num(&self.state.context, "baseLatency", 0.)
                + num(&self.state.context, "outputLatency", 0.))
    }
    pub fn build(&self, state: JsValue) {
        self.state.queue(state);
    }
    #[wasm_bindgen(js_name=applyParams)]
    pub fn apply_params(&self, state: JsValue) {
        self.state.queue(state);
    }
    #[wasm_bindgen(js_name=loadDI)]
    pub async fn load_di(&self, src: JsValue, name: Option<String>) -> Result<(), JsValue> {
        let name = name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "DI".into());
        let buffer = if let Some(url) = src.as_string() {
            let r = resolved(web_sys::window().unwrap().fetch_with_str(&url).into()).await?;
            if get(&r, "ok").as_bool() == Some(false) {
                return Err(js_sys::Error::new(&format!(
                    "Ошибка чтения DI ({})",
                    num(&r, "status", 0.)
                ))
                .into());
            }
            resolved(invoke(&r, "arrayBuffer", &[])?).await?
        } else {
            src
        };
        let api = get(&get(&web_sys::window().unwrap(), "__TAURI__"), "core");
        let headers = Object::new();
        set(
            &headers,
            "x-name",
            &js_sys::encode_uri_component(&name).into(),
        );
        let opts = Object::new();
        set(&opts, "headers", &headers);
        resolved(invoke(
            &api,
            "invoke",
            &[
                "engine_di_bytes".into(),
                Uint8Array::new(&buffer).into(),
                opts.into(),
            ],
        )?)
        .await?;
        self.state.di_name.replace(name);
        Ok(())
    }
    #[wasm_bindgen(js_name=loadDIPath)]
    pub async fn load_di_path(&self, path: String) -> Result<(), JsValue> {
        let r = cmd("engine_di_path", &json!({"path":path})).await?;
        self.state.di_name.replace(string(&r, "name"));
        Ok(())
    }
    #[wasm_bindgen(js_name=playDI)]
    pub fn play_di(&self) {
        self.state.playing.set(true);
        flag("engine_di_play", "on", json!(true));
    }
    #[wasm_bindgen(js_name=stopDI)]
    pub fn stop_di(&self) {
        self.state.playing.set(false);
        flag("engine_di_play", "on", json!(false));
    }
    #[wasm_bindgen(js_name=useLive)]
    pub async fn use_live(
        &self,
        id: Option<String>,
        channel: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        self.stop_di();
        self.state
            .configure(json!({"mode":"live","input":id,"channel":channel.unwrap_or(0)}))
            .await?;
        Ok(self.live_info())
    }
    #[wasm_bindgen(js_name=stopLive)]
    pub fn stop_live(&self) {
        if self.state.config.borrow()["mode"] == "live" {
            let s = self.state.clone();
            spawn_local(async move {
                if let Err(e) = s.configure(json!({"mode":"di"})).await {
                    web_sys::console::warn_1(&e);
                }
            });
        }
    }
    pub async fn devices(&self) -> Result<JsValue, JsValue> {
        let d: Value = read(command("engine_devices", &JsValue::UNDEFINED).await?)?;
        let names = |k: &str| {
            gc_model::catalog::list(&d[k])
                .iter()
                .map(|v| json!({"id":v,"label":v}))
                .collect::<Vec<_>>()
        };
        js(
            &json!({"inputs":names("inputs"),"outputs":names("outputs"),"labelsHidden":false,"defaultInput":d["defaultInput"],"defaultOutput":d["defaultOutput"]}),
        )
    }
    #[wasm_bindgen(js_name=setOutput)]
    pub async fn set_output(&self, id: String) -> Result<String, JsValue> {
        self.state.configure(json!({"output":id})).await?;
        Ok(id)
    }
    #[wasm_bindgen(js_name=testTone)]
    pub async fn test_tone(&self) -> Result<(), JsValue> {
        cmd("engine_test_tone", &json!({})).await.map(drop)
    }
    #[wasm_bindgen(js_name=setMetronome)]
    pub fn set_metronome(&self, on: bool, bpm: Option<f64>, level: Option<f64>) {
        self.state.metronome.set(on);
        self.set_bpm(bpm.unwrap_or(120.));
        fire(
            "engine_metronome",
            json!({"on":on,"bpm":self.bpm(),"level":level.unwrap_or(0.5)}),
        );
    }
    #[wasm_bindgen(js_name=setMuted)]
    pub fn set_muted(&self, on: bool) {
        self.state.muted.set(on);
        flag("engine_mute", "on", json!(on));
    }
    pub fn peak(&self, which: &str) -> f64 {
        num(
            &self.state.meters.borrow(),
            match which {
                "raw" => "raw",
                "input" => "input",
                _ => "output",
            },
            0.,
        )
    }
    #[wasm_bindgen(js_name=spectrumDb)]
    pub fn spectrum_db(&self) -> JsValue {
        self.state.spectrum_request.set(now());
        self.state.spectrum.borrow().clone()
    }
    #[wasm_bindgen(js_name=eqCurve)]
    pub async fn eq_curve(&self, eq: JsValue, freqs: JsValue) -> Result<JsValue, JsValue> {
        let a = Object::new();
        set(&a, "eq", &eq);
        set(&a, "freqs", &Array::from(&freqs));
        command("engine_eq_curve", &a).await
    }
    pub async fn analyze(&self, seconds: Option<f64>) -> Result<JsValue, JsValue> {
        cmd("engine_analyze", &json!({"seconds":seconds.unwrap_or(1.5)})).await
    }
    #[wasm_bindgen(js_name=tunerReading)]
    pub async fn tuner_reading(&self, a4: f64) -> Result<JsValue, JsValue> {
        cmd("engine_tuner", &json!({"a4":a4})).await
    }
    #[wasm_bindgen(js_name=startRecording)]
    pub async fn start_recording(&self) -> Result<JsValue, JsValue> {
        let r = cmd(
            "record_start",
            &json!({"name":format!("__rec_{}.wav",js_sys::Date::now() as u64)}),
        )
        .await?;
        js(&json!({"wall":num(&r,"wall",0.),"perf":now(),"audioTime":0}))
    }
    #[wasm_bindgen(js_name=stopRecording)]
    pub async fn stop_recording(&self, name: Option<String>) -> Result<JsValue, JsValue> {
        cmd("record_stop", &json!({"name":name})).await
    }
}
