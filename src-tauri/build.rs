use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for path in [
        "app/index.html",
        "app/styles.css",
        "app/assets",
        "src-tauri/icons/128x128.png",
        "crates/gc-model",
        "crates/gc-ui",
        "Cargo.lock",
        "Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let target = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("frontend");
    let wasm = target.join("wasm32-unknown-unknown/release/gc_ui.wasm");
    let status = Command::new(env::var_os("CARGO").unwrap())
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            "gc-ui",
            "--target-dir",
        ])
        .arg(&target)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("compile GuitarCraft UI");
    assert!(status.success(),"Failed to build the Rust UI. Install its target with rustup target add wasm32-unknown-unknown");
    let frontend = root.join("app/wasm");
    wasm_bindgen_cli_support::Bindgen::new()
        .input_path(&wasm)
        .web(true)
        .unwrap()
        .typescript(false)
        .generate(&frontend)
        .expect("generate WebView bindings");
    fs::copy(
        root.join("src-tauri/icons/128x128.png"),
        frontend.join("app-icon.png"),
    )
    .expect("copy the application icon to the WebView");
    fs::write(
        frontend.join("boot.js"),
        r#"import('./gc_ui.js')
  .then(({ default: init, start }) => init({ module_or_path: new URL('./gc_ui_bg.wasm', import.meta.url) }).then(start))
  .catch(error => {
    const message = document.createElement('pre');
    message.className = 'boot-error';
    message.textContent = error.message || String(error);
    document.body.replaceChildren(message);
    console.error(error);
  });
"#,
    )
    .expect("generate WebAssembly loader");
    tauri_build::build();
}
