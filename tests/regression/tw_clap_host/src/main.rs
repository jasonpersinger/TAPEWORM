//! tw_clap_host — independent black-box CLAP host for the TAPEWORM certification.
//!
//! Loads a shipped `.clap` binary via `clap_entry` and drives it only through the
//! CLAP ABI. Every subcommand runs single-threaded on the calling thread.
//!
//!   info      <plugin> <sr>
//!   render    <plugin> <sr> <block> <in.f32> <out.f32> <script>
//!   bench     <plugin> <sr> <block> <nblocks> <ninst> <script> <out.csv>
//!   state     <pluginA> <pluginB> <sr> <script> <state.bin>
//!   loadstate <plugin> <state.bin>
//!   lifecycle <plugin> <cycles>
//!
//! Script lines (plain values, names as reported by clap_param_info.name):
//!   init <Param Name>=<value>          applied with params.flush() before activate
//!   at <sample> <Param Name>=<value>   sample-accurate CLAP_EVENT_PARAM_VALUE
//!   signal noise|silence|sine:<hz>:<amp>   (bench only)
//!   auto <Param Name>=<lo>:<hi>        (bench only) 4 events per block per param

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::latency::{clap_plugin_latency, CLAP_EXT_LATENCY};
use clap_sys::ext::params::*;
use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::clap_process;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use std::ffi::{c_char, c_void, CStr, CString};
use std::time::Instant;

// ---------------------------------------------------------------- host vtable
unsafe extern "C" fn h_ext(_: *const clap_host, _: *const c_char) -> *const c_void {
    std::ptr::null()
}
unsafe extern "C" fn h_nop(_: *const clap_host) {}

fn make_host() -> &'static clap_host {
    Box::leak(Box::new(clap_host {
        clap_version: CLAP_VERSION,
        host_data: std::ptr::null_mut(),
        name: c"tw_clap_host (certification)".as_ptr(),
        vendor: c"independent".as_ptr(),
        url: c"".as_ptr(),
        version: c"0.1".as_ptr(),
        get_extension: Some(h_ext),
        request_restart: Some(h_nop),
        request_process: Some(h_nop),
        request_callback: Some(h_nop),
    }))
}

// ---------------------------------------------------------------- event lists
struct InEvents {
    list: clap_input_events,
    ev: Vec<clap_event_param_value>,
}
unsafe extern "C" fn in_size(l: *const clap_input_events) -> u32 {
    (*((*l).ctx as *const Vec<clap_event_param_value>)).len() as u32
}
unsafe extern "C" fn in_get(l: *const clap_input_events, i: u32) -> *const clap_event_header {
    let v: &Vec<clap_event_param_value> = &*((*l).ctx as *const Vec<clap_event_param_value>);
    &v[i as usize].header
}
impl InEvents {
    fn new() -> Box<Self> {
        let mut b = Box::new(InEvents {
            list: clap_input_events { ctx: std::ptr::null_mut(), size: Some(in_size), get: Some(in_get) },
            ev: Vec::with_capacity(4096),
        });
        b.list.ctx = &b.ev as *const _ as *mut c_void;
        b
    }
}
fn pv(time: u32, id: u32, value: f64) -> clap_event_param_value {
    clap_event_param_value {
        header: clap_event_header {
            size: std::mem::size_of::<clap_event_param_value>() as u32,
            time,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_PARAM_VALUE,
            flags: 0,
        },
        param_id: id,
        cookie: std::ptr::null_mut(),
        note_id: -1,
        port_index: -1,
        channel: -1,
        key: -1,
        value,
    }
}
unsafe extern "C" fn out_push(_: *const clap_output_events, _: *const clap_event_header) -> bool {
    true
}
struct SyncOut(clap_output_events);
unsafe impl Sync for SyncOut {}
static OUT_EVENTS: SyncOut = SyncOut(clap_output_events { ctx: std::ptr::null_mut(), try_push: Some(out_push) });
/// Static so that the host performs no allocation inside the measured process() region.
fn out_events() -> &'static clap_output_events {
    &OUT_EVENTS.0
}

/// Allocation counter exported by alloc_shim.so when LD_PRELOADed; None otherwise.
fn alloc_counter() -> Option<unsafe extern "C" fn() -> u64> {
    unsafe {
        let p = libc_dlsym(std::ptr::null_mut(), c"tw_alloc_count".as_ptr());
        if p.is_null() { None } else { Some(std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> u64>(p)) }
    }
}
extern "C" {
    #[link_name = "dlsym"]
    fn libc_dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

// ---------------------------------------------------------------- library
struct Lib {
    _lib: libloading::Library,
    entry: *const clap_plugin_entry,
    factory: *const clap_plugin_factory,
}
impl Lib {
    fn load(path: &str) -> Lib {
        unsafe {
            let lib = libloading::Library::new(path).expect("dlopen failed");
            let sym: libloading::Symbol<*const clap_plugin_entry> = lib.get(b"clap_entry\0").expect("no clap_entry");
            let entry = *sym;
            let cpath = CString::new(path).unwrap();
            assert!(((*entry).init.unwrap())(cpath.as_ptr()), "clap_entry.init failed");
            let factory = ((*entry).get_factory.unwrap())(CLAP_PLUGIN_FACTORY_ID.as_ptr()) as *const clap_plugin_factory;
            assert!(!factory.is_null(), "no plugin factory");
            Lib { _lib: lib, entry, factory }
        }
    }
    fn plugin_id(&self) -> CString {
        unsafe {
            let n = ((*self.factory).get_plugin_count.unwrap())(self.factory);
            assert_eq!(n, 1, "expected exactly one plugin in factory");
            let d = ((*self.factory).get_plugin_descriptor.unwrap())(self.factory, 0);
            CStr::from_ptr((*d).id).to_owned()
        }
    }
    fn describe(&self) {
        unsafe {
            let d = ((*self.factory).get_plugin_descriptor.unwrap())(self.factory, 0);
            let s = |p: *const c_char| if p.is_null() { "(null)".to_string() } else { CStr::from_ptr(p).to_string_lossy().into_owned() };
            println!("descriptor.clap_version\t{}.{}.{}", (*d).clap_version.major, (*d).clap_version.minor, (*d).clap_version.revision);
            println!("descriptor.id\t{}", s((*d).id));
            println!("descriptor.name\t{}", s((*d).name));
            println!("descriptor.vendor\t{}", s((*d).vendor));
            println!("descriptor.url\t{}", s((*d).url));
            println!("descriptor.manual_url\t{}", s((*d).manual_url));
            println!("descriptor.support_url\t{}", s((*d).support_url));
            println!("descriptor.version\t{}", s((*d).version));
            println!("descriptor.description\t{}", s((*d).description));
            let mut f = (*d).features;
            let mut feats = vec![];
            while !f.is_null() && !(*f).is_null() {
                feats.push(s(*f));
                f = f.add(1);
            }
            println!("descriptor.features\t{}", feats.join(","));
        }
    }
}
impl Drop for Lib {
    fn drop(&mut self) {
        unsafe { ((*self.entry).deinit.unwrap())() }
    }
}

// ---------------------------------------------------------------- instance
#[derive(Clone)]
struct ParamInfo {
    id: u32,
    name: String,
    min: f64,
    max: f64,
    default: f64,
    flags: u32,
}

struct Inst {
    p: *const clap_plugin,
    params: *const clap_plugin_params,
    state: *const clap_plugin_state,
    latency: *const clap_plugin_latency,
    infos: Vec<ParamInfo>,
    active: bool,
}
impl Inst {
    fn new(lib: &Lib) -> Inst {
        unsafe {
            let id = lib.plugin_id();
            let p = ((*lib.factory).create_plugin.unwrap())(lib.factory, make_host(), id.as_ptr());
            assert!(!p.is_null(), "create_plugin failed");
            assert!(((*p).init.unwrap())(p), "plugin.init failed");
            let ge = (*p).get_extension.unwrap();
            let params = ge(p, CLAP_EXT_PARAMS.as_ptr()) as *const clap_plugin_params;
            let state = ge(p, CLAP_EXT_STATE.as_ptr()) as *const clap_plugin_state;
            let latency = ge(p, CLAP_EXT_LATENCY.as_ptr()) as *const clap_plugin_latency;
            let mut me = Inst { p, params, state, latency, infos: vec![], active: false };
            me.read_infos();
            me
        }
    }
    fn read_infos(&mut self) {
        unsafe {
            let n = ((*self.params).count.unwrap())(self.p);
            for i in 0..n {
                let mut info: clap_param_info = std::mem::zeroed();
                assert!(((*self.params).get_info.unwrap())(self.p, i, &mut info));
                self.infos.push(ParamInfo {
                    id: info.id,
                    name: CStr::from_ptr(info.name.as_ptr()).to_string_lossy().into_owned(),
                    min: info.min_value,
                    max: info.max_value,
                    default: info.default_value,
                    flags: info.flags,
                });
            }
        }
    }
    fn id_of(&self, name: &str) -> u32 {
        self.infos.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("unknown param {name:?}")).id
    }
    fn value(&self, id: u32) -> f64 {
        let mut v = f64::NAN;
        unsafe { ((*self.params).get_value.unwrap())(self.p, id, &mut v) };
        v
    }
    fn text(&self, id: u32, v: f64) -> String {
        let mut buf = [0 as c_char; 256];
        unsafe {
            if ((*self.params).value_to_text.unwrap())(self.p, id, v, buf.as_mut_ptr(), 256) {
                CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
            } else {
                "(no text)".into()
            }
        }
    }
    fn flush(&self, evs: &[(u32, f64)]) {
        let mut ie = InEvents::new();
        for &(id, v) in evs {
            ie.ev.push(pv(0, id, v));
        }
        unsafe { ((*self.params).flush.unwrap())(self.p, &ie.list, out_events()) };
    }
    fn activate(&mut self, sr: f64, max_frames: u32) {
        unsafe {
            assert!(((*self.p).activate.unwrap())(self.p, sr, 1, max_frames), "activate failed");
            assert!(((*self.p).start_processing.unwrap())(self.p), "start_processing failed");
        }
        self.active = true;
    }
    fn deactivate(&mut self) {
        if self.active {
            unsafe {
                ((*self.p).stop_processing.unwrap())(self.p);
                ((*self.p).deactivate.unwrap())(self.p);
            }
            self.active = false;
        }
    }
    fn latency(&self) -> u32 {
        if self.latency.is_null() {
            return u32::MAX;
        }
        unsafe { ((*self.latency).get.unwrap())(self.p) }
    }
    /// Process one block: `l`/`r` are input, `ol`/`or` receive output.
    fn process(&self, steady: i64, l: &mut [f32], r: &mut [f32], ev: &InEvents, ol: &mut [f32], or: &mut [f32]) -> i32 {
        let n = l.len() as u32;
        let mut inp = [l.as_mut_ptr(), r.as_mut_ptr()];
        let mut outp = [ol.as_mut_ptr(), or.as_mut_ptr()];
        let ib = clap_audio_buffer { data32: inp.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
        let mut ob = clap_audio_buffer { data32: outp.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
        let proc_ = clap_process {
            steady_time: steady,
            frames_count: n,
            transport: std::ptr::null(),
            audio_inputs: &ib,
            audio_outputs: &mut ob,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: &ev.list,
            out_events: out_events(),
        };
        unsafe { ((*self.p).process.unwrap())(self.p, &proc_) }
    }
    fn save(&self) -> Vec<u8> {
        unsafe extern "C" fn w(s: *const clap_ostream, buf: *const c_void, n: u64) -> i64 {
            let v = &mut *((*s).ctx as *mut Vec<u8>);
            v.extend_from_slice(std::slice::from_raw_parts(buf as *const u8, n as usize));
            n as i64
        }
        let mut out: Vec<u8> = vec![];
        let os = clap_ostream { ctx: &mut out as *mut _ as *mut c_void, write: Some(w) };
        assert!(unsafe { ((*self.state).save.unwrap())(self.p, &os) }, "state.save failed");
        out
    }
    fn load(&self, data: &[u8]) -> bool {
        struct Cur<'a> {
            d: &'a [u8],
            pos: usize,
        }
        unsafe extern "C" fn r(s: *const clap_istream, buf: *mut c_void, n: u64) -> i64 {
            let c = &mut *((*s).ctx as *mut Cur);
            let k = (n as usize).min(c.d.len() - c.pos);
            std::ptr::copy_nonoverlapping(c.d[c.pos..].as_ptr(), buf as *mut u8, k);
            c.pos += k;
            k as i64
        }
        let mut cur = Cur { d: data, pos: 0 };
        let is = clap_istream { ctx: &mut cur as *mut _ as *mut c_void, read: Some(r) };
        unsafe { ((*self.state).load.unwrap())(self.p, &is) }
    }
}
impl Drop for Inst {
    fn drop(&mut self) {
        self.deactivate();
        unsafe { ((*self.p).destroy.unwrap())(self.p) }
    }
}

// ---------------------------------------------------------------- script
#[derive(Default)]
struct Script {
    init: Vec<(String, f64)>,
    at: Vec<(u64, String, f64)>,
    signal: String,
    auto: Vec<(String, f64, f64)>,
}
fn parse_script(path: &str) -> Script {
    let mut s = Script { signal: "noise".into(), ..Default::default() };
    if path == "-" {
        return s;
    }
    for line in std::fs::read_to_string(path).expect("read script").lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kw, rest) = line.split_once(' ').unwrap();
        let kv = |t: &str| {
            let (k, v) = t.rsplit_once('=').unwrap();
            (k.trim().to_string(), v.trim().to_string())
        };
        match kw {
            "init" => {
                let (k, v) = kv(rest);
                s.init.push((k, v.parse().unwrap()))
            }
            "at" => {
                let (t, kvs) = rest.split_once(' ').unwrap();
                let (k, v) = kv(kvs);
                s.at.push((t.parse().unwrap(), k, v.parse().unwrap()))
            }
            "signal" => s.signal = rest.trim().to_string(),
            "auto" => {
                let (k, v) = kv(rest);
                let (lo, hi) = v.split_once(':').unwrap();
                s.auto.push((k, lo.parse().unwrap(), hi.parse().unwrap()))
            }
            _ => panic!("bad script line {line}"),
        }
    }
    s.at.sort_by_key(|e| e.0);
    s
}
fn apply_init(inst: &Inst, s: &Script) {
    let evs: Vec<(u32, f64)> = s.init.iter().map(|(k, v)| (inst.id_of(k), *v)).collect();
    inst.flush(&evs);
}

fn read_f32(path: &str) -> Vec<f32> {
    let b = std::fs::read(path).expect("read input");
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

// ---------------------------------------------------------------- commands
fn cmd_info(a: &[String]) {
    let lib = Lib::load(&a[0]);
    lib.describe();
    let sr: f64 = a[1].parse().unwrap();
    let mut inst = Inst::new(&lib);
    println!("params.count\t{}", inst.infos.len());
    println!("#idx\tid\tname\tmin\tmax\tdefault\tflags\tdefault_text\tmin_text\tmax_text");
    for (i, p) in inst.infos.iter().enumerate() {
        println!(
            "param\t{i}\t{}\t{}\t{}\t{}\t{}\t0x{:x}\t{}\t{}\t{}",
            p.id, p.name, p.min, p.max, p.default, p.flags,
            inst.text(p.id, p.default), inst.text(p.id, p.min), inst.text(p.id, p.max)
        );
    }
    inst.activate(sr, 1024);
    println!("latency_samples@{sr}\t{}", inst.latency());
    println!("has_state_ext\t{}", !inst.state.is_null());
}

fn cmd_render(a: &[String]) {
    let lib = Lib::load(&a[0]);
    let sr: f64 = a[1].parse().unwrap();
    let block: usize = a[2].parse().unwrap();
    let input = read_f32(&a[3]);
    let script = parse_script(&a[5]);
    let mut inst = Inst::new(&lib);
    apply_init(&inst, &script);
    inst.activate(sr, block as u32);
    let frames = input.len() / 2;
    let mut out = vec![0f32; frames * 2];
    let (mut l, mut r, mut ol, mut or) = (vec![0f32; block], vec![0f32; block], vec![0f32; block], vec![0f32; block]);
    let mut ev = InEvents::new();
    let mut next_ev = 0usize;
    let mut pos = 0usize;
    while pos < frames {
        let n = block.min(frames - pos);
        for i in 0..n {
            l[i] = input[2 * (pos + i)];
            r[i] = input[2 * (pos + i) + 1];
        }
        ev.ev.clear();
        while next_ev < script.at.len() && (script.at[next_ev].0 as usize) < pos + n {
            let (t, k, v) = &script.at[next_ev];
            ev.ev.push(pv((*t as usize - pos) as u32, inst.id_of(k), *v));
            next_ev += 1;
        }
        let st = inst.process(pos as i64, &mut l[..n], &mut r[..n], &ev, &mut ol[..n], &mut or[..n]);
        assert!(st != 0, "process returned CLAP_PROCESS_ERROR");
        for i in 0..n {
            out[2 * (pos + i)] = ol[i];
            out[2 * (pos + i) + 1] = or[i];
        }
        pos += n;
    }
    let bytes: Vec<u8> = out.iter().flat_map(|x| x.to_le_bytes()).collect();
    std::fs::write(&a[4], bytes).unwrap();
    eprintln!("rendered {frames} frames @ {sr} Hz, block {block}, latency {}", inst.latency());
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn cmd_bench(a: &[String]) {
    let lib = Lib::load(&a[0]);
    let sr: f64 = a[1].parse().unwrap();
    let block: usize = a[2].parse().unwrap();
    let nblocks: usize = a[3].parse().unwrap();
    let ninst: usize = a[4].parse().unwrap();
    let script = parse_script(&a[5]);
    let rss0 = rss_kb();
    let pc0 = alloc_counter().map(|f| unsafe { f() }).unwrap_or(0);
    let mut insts: Vec<Inst> = (0..ninst).map(|_| Inst::new(&lib)).collect();
    for i in insts.iter_mut() {
        apply_init(i, &script);
        i.activate(sr, block as u32);
    }
    let pc1 = alloc_counter().map(|f| unsafe { f() }).unwrap_or(0);
    // Positive control: instance creation/activation must register allocations.
    println!("positive_control_allocs_during_create_activate={}", pc1 - pc0);
    let auto_ids: Vec<(u32, f64, f64)> = script.auto.iter().map(|(k, lo, hi)| (insts[0].id_of(k), *lo, *hi)).collect();
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut l = vec![0f32; block];
    let mut r = vec![0f32; block];
    let mut ol = vec![0f32; block];
    let mut or = vec![0f32; block];
    let mut ev = InEvents::new();
    let mut times = Vec::with_capacity(nblocks);
    let mut phase = 0f64;
    let mut nonfinite = 0u64;
    let counter = alloc_counter();
    let mut allocs_in_process = 0u64;
    let rss1 = rss_kb();
    for b in 0..nblocks {
        ev.ev.clear();
        for (j, &(id, lo, hi)) in auto_ids.iter().enumerate() {
            for k in 0..4 {
                let t = ((k * block) / 4 + j) % block;
                let x = (rng.next() as f64 + 1.0) * 0.5;
                ev.ev.push(pv(t as u32, id, lo + (hi - lo) * x));
            }
        }
        ev.ev.sort_by_key(|e| e.header.time);
        let t0 = Instant::now();
        for inst in insts.iter() {
            for i in 0..block {
                let x = match script.signal.as_str() {
                    "silence" => 0.0,
                    "noise" => 0.25 * rng.next(),
                    s => {
                        let mut it = s.split(':').skip(1);
                        let hz: f64 = it.next().unwrap().parse().unwrap();
                        let amp: f64 = it.next().unwrap().parse().unwrap();
                        phase += hz / sr;
                        (amp * (2.0 * std::f64::consts::PI * phase).sin()) as f32
                    }
                };
                l[i] = x;
                r[i] = x;
            }
            let c0 = counter.map(|f| unsafe { f() }).unwrap_or(0);
            inst.process((b * block) as i64, &mut l, &mut r, &ev, &mut ol, &mut or);
            let c1 = counter.map(|f| unsafe { f() }).unwrap_or(0);
            allocs_in_process += c1 - c0;
        }
        times.push(t0.elapsed().as_nanos() as u64);
        if ol.iter().chain(or.iter()).any(|v| !v.is_finite()) {
            nonfinite += 1;
        }
    }
    let rss2 = rss_kb();
    // Note: measured time includes host-side input generation for all instances
    // (a few ns/sample); this makes the figures conservative.
    let mut csv = String::from("block_index,callback_ns\n");
    for (i, t) in times.iter().enumerate() {
        csv.push_str(&format!("{i},{t}\n"));
    }
    std::fs::write(&a[6], csv).unwrap();
    let alloc_txt = if counter.is_some() { allocs_in_process.to_string() } else { "not_measured".into() };
    println!("rss_kb_before_create={rss0} after_activate={rss1} after_run={rss2} nonfinite_blocks={nonfinite} allocs_or_frees_inside_process={alloc_txt}");
}

fn cmd_state(a: &[String]) {
    let lib_a = Lib::load(&a[0]);
    let lib_b = Lib::load(&a[1]);
    let sr: f64 = a[2].parse().unwrap();
    let script = parse_script(&a[3]);
    let mut ia = Inst::new(&lib_a);
    apply_init(&ia, &script);
    ia.activate(sr, 512);
    let blob = ia.save();
    std::fs::write(&a[4], &blob).unwrap();
    println!("saved_bytes\t{}", blob.len());
    let mut ib = Inst::new(&lib_b);
    let ok = ib.load(&blob);
    println!("load_ok\t{ok}");
    ib.activate(sr, 512);
    let mut mism = 0;
    for p in &ia.infos {
        let va = ia.value(p.id);
        match ib.infos.iter().find(|q| q.id == p.id) {
            None => {
                println!("MISSING_IN_B\t{}\t{}", p.id, p.name);
                mism += 1;
            }
            Some(q) => {
                let vb = ib.value(q.id);
                let same = (va - vb).abs() <= 1e-9 * va.abs().max(1.0);
                if !same {
                    mism += 1;
                }
                println!("param\t{}\t{}\tA={va}\tB={vb}\t{}", p.id, p.name, if same { "OK" } else { "MISMATCH" });
            }
        }
    }
    // Audio identity after state transfer: identical noise through both.
    let block = 512;
    let mut rng = Rng(42);
    let mut maxdiff = 0f32;
    let (mut la, mut ra, mut lb, mut rb) = (vec![0f32; block], vec![0f32; block], vec![0f32; block], vec![0f32; block]);
    let (mut oa, mut pa, mut ob, mut pb) = (vec![0f32; block], vec![0f32; block], vec![0f32; block], vec![0f32; block]);
    let ev = InEvents::new();
    for b in 0..400 {
        for i in 0..block {
            let x = 0.3 * rng.next();
            la[i] = x;
            ra[i] = -x;
            lb[i] = x;
            rb[i] = -x;
        }
        ia.process((b * block) as i64, &mut la, &mut ra, &ev, &mut oa, &mut pa);
        ib.process((b * block) as i64, &mut lb, &mut rb, &ev, &mut ob, &mut pb);
        for i in 0..block {
            maxdiff = maxdiff.max((oa[i] - ob[i]).abs()).max((pa[i] - pb[i]).abs());
        }
    }
    println!("param_mismatches\t{mism}");
    println!("audio_max_abs_diff_after_transfer\t{maxdiff:e}");
}

fn cmd_loadstate(a: &[String]) {
    let lib = Lib::load(&a[0]);
    let blob = std::fs::read(&a[1]).unwrap();
    let inst = Inst::new(&lib);
    println!("load_ok\t{}", inst.load(&blob));
    for p in &inst.infos {
        let v = inst.value(p.id);
        println!("param\t{}\t{}\t{v}\t{}", p.id, p.name, inst.text(p.id, v));
    }
}

fn cmd_lifecycle(a: &[String]) {
    let path = &a[0];
    let cycles: usize = a[1].parse().unwrap();
    let rates = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0];
    let blocks = [1usize, 7, 32, 64, 333, 512, 1024];
    let mut rng = Rng(7);
    let mut worst = 0f32;
    let mut nonfinite = 0u64;
    let rss0 = rss_kb();
    for c in 0..cycles {
        let lib = Lib::load(path); // dlopen + clap_entry.init each cycle
        let mut insts: Vec<Inst> = (0..3).map(|_| Inst::new(&lib)).collect();
        for (k, inst) in insts.iter_mut().enumerate() {
            for (j, &sr) in rates.iter().enumerate() {
                let block = blocks[(c + j + k) % blocks.len()];
                inst.activate(sr, block as u32); // sample-rate change = deactivate/activate
                let exp = (0.015f64 * sr).round() as u32;
                assert_eq!(inst.latency(), exp, "latency at {sr}");
                let (mut l, mut r, mut ol, mut or) = (vec![0f32; block], vec![0f32; block], vec![0f32; block], vec![0f32; block]);
                let ev = InEvents::new();
                for b in 0..((sr as usize / 10) / block).max(4) {
                    for i in 0..block {
                        l[i] = 0.9 * rng.next();
                        r[i] = 0.9 * rng.next();
                    }
                    inst.process((b * block) as i64, &mut l, &mut r, &ev, &mut ol, &mut or);
                    for v in ol.iter().chain(or.iter()) {
                        if !v.is_finite() {
                            nonfinite += 1;
                        }
                        worst = worst.max(v.abs());
                    }
                }
                inst.deactivate();
            }
        }
        drop(insts);
        drop(lib); // clap_entry.deinit + dlclose
    }
    println!("cycles={cycles} instances_per_cycle=3 rates={rates:?} nonfinite_samples={nonfinite} peak_out={worst} rss_kb_start={rss0} rss_kb_end={}", rss_kb());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rest = &args[2..];
    match args[1].as_str() {
        "info" => cmd_info(rest),
        "render" => cmd_render(rest),
        "bench" => cmd_bench(rest),
        "state" => cmd_state(rest),
        "loadstate" => cmd_loadstate(rest),
        "lifecycle" => cmd_lifecycle(rest),
        c => panic!("unknown command {c}"),
    }
}
