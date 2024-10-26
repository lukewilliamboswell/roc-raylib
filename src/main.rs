use matchbox_socket::{PeerId, PeerState};
use platform_mode::PlatformEffect;
use roc_std::{RocBox, RocList, RocResult, RocStr};
use roc_std_heap::ThreadSafeRefcountedResourceHeap;
use std::array;
use std::collections::HashMap;
use std::ffi::{c_int, CString};
use std::time::SystemTime;

mod bindings;
mod config;
mod glue;
mod platform_mode;
mod roc;
mod worker;

/// use different error codes when the app exits
#[derive(Debug)]
enum ExitErrCode {
    ExitEffectNotPermitted = 1,
    ExitHeapFull = 2,
}

const MAIN_TO_WORKER_BUFFER_SIZE: usize = 100;
const WORKER_TO_MAIN_BUFFER_SIZE: usize = 1000;

fn main() {
    // CALL INTO ROC FOR INITALIZATION
    let mut model = roc::call_roc_init();

    // MANUALLY TRANSITION TO RENDER MODE
    platform_mode::update(PlatformEffect::EndInitWindow).unwrap();

    let mut frame_count = 0;

    let (main_tx, worker_rx) =
        tokio::sync::mpsc::channel::<worker::MainToWorkerMsg>(MAIN_TO_WORKER_BUFFER_SIZE);
    let (worker_tx, mut main_rx) =
        tokio::sync::mpsc::channel::<worker::WorkerToMainMsg>(WORKER_TO_MAIN_BUFFER_SIZE);

    // TODO move this into a separate thread
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let worker_handle = rt.spawn(worker::worker_loop(worker_rx, worker_tx));
    let mut peers: HashMap<PeerId, PeerState> = HashMap::new();

    unsafe {
        'render_loop: while !bindings::WindowShouldClose() && !(config::with(|c| c.should_exit)) {
            // Try to receive any pending (non-blocking)
            while let Ok(msg) = main_rx.try_recv() {
                // use worker::ConnectionState::*;
                use worker::WorkerToMainMsg::*;
                match msg {
                    Tock => {
                        // println!("Received Tock from worker");
                    }
                    PeerConnected(peer) => {
                        peers.insert(peer, PeerState::Connected);
                    }
                    PeerDisconnected(peer) => {
                        peers.insert(peer, PeerState::Disconnected);
                    }
                    MessageReceived(peer, data) => {
                        let message = String::from_utf8_lossy(&data);
                        println!("Main: Message from {peer}: {message:?}");
                    }
                    Error(error) => {
                        println!("Main: Worker error: {error}");
                        // Optionally handle worker errors (e.g., reconnect logic)
                    } // ConnectionStatus(status) => match status {
                      //     Connected => {
                      //         println!("Main: Worker connected");
                      //     }
                      //     Disconnected(msg) => {
                      //         println!("Main: Worker disconnected: {msg}");
                      //     }
                      //     Failed(msg) => {
                      //         println!("Main: Worker connection failed: {msg}");
                      //     }
                      // },
                }
            }

            let duration_since_epoch = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap();

            if config::with(|c| c.fps_target_dirty) {
                bindings::SetTargetFPS(config::with(|c| c.fps_target));
                config::update(|c| c.fps_target_dirty = false);
            }

            let timestamp = duration_since_epoch.as_millis() as u64; // we are casting to u64 and losing precision

            trace_log(&format!(
                "RENDER frame: {}, millis: {} ------",
                frame_count, timestamp
            ));

            let platform_state = glue::PlatformState {
                frame_count,
                peers: (&peers).into(),
                keys: get_keys_states(),
                messages: RocList::empty(),
                mouse_buttons: get_mouse_button_states(),
                timestamp_millis: timestamp,
                mouse_pos_x: bindings::GetMouseX() as f32,
                mouse_pos_y: bindings::GetMouseY() as f32,
                mouse_wheel: bindings::GetMouseWheelMove(),
            };

            model = roc::call_roc_render(platform_state, &model);

            if config::with(|c| c.fps_show) {
                config::with(|c| bindings::DrawFPS(c.fps_position.0, c.fps_position.1));
            }

            frame_count += 1;

            bindings::EndDrawing();

            // Send Tick message to worker (non-blocking)
            if main_tx.try_send(worker::MainToWorkerMsg::Tick).is_err() {
                println!("Worker thread has disconnected");
                break 'render_loop;
            }
        }

        // Send shutdown message BEFORE closing the window
        println!("Sending shutdown signal to worker...");
        if let Err(e) = rt.block_on(main_tx.send(worker::MainToWorkerMsg::Shutdown)) {
            println!("Failed to send shutdown signal: {:?}", e);
        }

        // Wait for worker task to complete
        println!("Waiting for worker task to finish...");
        if let Err(e) = rt.block_on(worker_handle) {
            println!("Worker task error: {:?}", e);
        }

        // Now safe to close the window
        println!("Closing window...");
        bindings::CloseWindow();
    }
}

/// exit the program with a message and a code, close the window
fn exit_with_msg(msg: String, code: ExitErrCode) -> ! {
    let c_msg = CString::new(msg).unwrap();
    unsafe {
        bindings::TraceLog(bindings::TraceLogLevel_LOG_FATAL as i32, c_msg.as_ptr());
        bindings::CloseWindow();
    }
    std::process::exit(code as i32);
}

#[no_mangle]
extern "C" fn roc_fx_exit() -> RocResult<(), ()> {
    config::update(|c| c.should_exit = true);
    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_log(msg: &RocStr, level: i32) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::LogMsg) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let text = CString::new(msg.as_str()).unwrap();
    if level >= 0 && level <= 7 {
        unsafe { bindings::TraceLog(level, text.as_ptr()) }
    } else {
        panic!("Invalid log level from roc");
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_initWindow(title: &RocStr, width: f32, height: f32) -> RocResult<(), ()> {
    config::update(|c| {
        c.title = CString::new(title.to_string()).unwrap();
        c.width = width as i32;
        c.height = height as i32;
    });

    if let Err(msg) = platform_mode::update(PlatformEffect::InitWindow) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let title = config::with(|c| c.title.as_ptr());
    let width = config::with(|c| c.width);
    let height = config::with(|c| c.height);

    unsafe {
        bindings::InitWindow(width, height, title);

        // wait for the window to be ready (blocking)
        if !bindings::IsWindowReady() {
            panic!("Attempting to create window failed!");
        }

        bindings::SetTraceLogLevel(config::with(|c| c.trace_log_level.into()));
        bindings::SetTargetFPS(config::with(|c| c.fps_target));

        bindings::InitAudioDevice();
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawCircle(
    center: &glue::RocVector2,
    radius: f32,
    color: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawCircle) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        bindings::DrawCircleV(center.into(), radius, color.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawCircleGradient(
    center: &glue::RocVector2,
    radius: f32,
    inner: glue::RocColor,
    outer: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawCircleGradient) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let (x, y) = center.to_components_c_int();

    unsafe {
        bindings::DrawCircleGradient(x, y, radius, inner.into(), outer.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawRectangleGradientV(
    rect: &glue::RocRectangle,
    top: glue::RocColor,
    bottom: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawRectangleGradientV) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let (x, y, w, h) = rect.to_components_c_int();

    unsafe {
        bindings::DrawRectangleGradientV(x, y, w, h, top.into(), bottom.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawRectangleGradientH(
    rect: &glue::RocRectangle,
    top: glue::RocColor,
    bottom: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawRectangleGradientH) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let (x, y, w, h) = rect.to_components_c_int();

    unsafe {
        bindings::DrawRectangleGradientV(x, y, w, h, top.into(), bottom.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawText(
    pos: &glue::RocVector2,
    size: i32,
    text: &RocStr,
    color: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawText) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let text = CString::new(text.as_bytes()).unwrap();
    let (x, y) = pos.to_components_c_int();

    unsafe {
        bindings::DrawText(text.as_ptr(), x, y, size as c_int, color.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawRectangle(
    rect: &glue::RocRectangle,
    color: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawRectangle) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        bindings::DrawRectangleRec(rect.into(), color.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawLine(
    start: &glue::RocVector2,
    end: &glue::RocVector2,
    color: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawLine) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        bindings::DrawLineV(start.into(), end.into(), color.into());
    }

    RocResult::ok(())
}

#[repr(C)]
struct ScreenSize {
    z: i64,
    height: i32,
    width: i32,
}

#[no_mangle]
extern "C" fn roc_fx_getScreenSize() -> RocResult<ScreenSize, ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::GetScreenSize) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        let height = bindings::GetScreenHeight();
        let width = bindings::GetScreenWidth();
        RocResult::ok(ScreenSize {
            height,
            width,
            z: 0,
        })
    }
}

#[no_mangle]
extern "C" fn roc_fx_measureText(text: &RocStr, size: i32) -> RocResult<i64, ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::MeasureText) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let text = CString::new(text.as_str()).unwrap();
    let width = unsafe { bindings::MeasureText(text.as_ptr(), size as c_int) };
    RocResult::ok(width as i64)
}

#[no_mangle]
extern "C" fn roc_fx_setTargetFPS(rate: i32) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::SetTargetFPS) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    config::update(|c| {
        c.fps_target_dirty = true;
        c.fps_target = rate as c_int
    });

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_takeScreenshot(path: &RocStr) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::TakeScreenshot) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let path = CString::new(path.as_str()).unwrap();

    unsafe {
        bindings::TakeScreenshot(path.as_ptr());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_setDrawFPS(show: bool, pos_x: i32, pos_y: i32) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::SetDrawFPS) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    config::update(|c| {
        c.fps_show = show;
        c.fps_position = (pos_x, pos_y)
    });

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_createCamera(
    target: &glue::RocVector2,
    offset: &glue::RocVector2,
    rotation: f32,
    zoom: f32,
) -> RocResult<RocBox<()>, ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::CreateCamera) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let camera = bindings::Camera2D {
        target: target.into(),
        offset: offset.into(),
        rotation,
        zoom,
    };

    let heap = roc::camera_heap();

    let alloc_result = heap.alloc_for(camera);
    match alloc_result {
        Ok(roc_box) => RocResult::ok(roc_box),
        Err(_) => {
            exit_with_msg("Unable to load camera, out of memory in the camera heap. Consider using ROC_RAY_MAX_CAMERAS_HEAP_SIZE env var to increase the heap size.".into(), ExitErrCode::ExitHeapFull);
        }
    }
}

#[no_mangle]
extern "C" fn roc_fx_createRenderTexture(size: &glue::RocVector2) -> RocResult<RocBox<()>, ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::CreateRenderTexture) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let (width, height) = size.to_components_c_int();

    let render_texture = unsafe { bindings::LoadRenderTexture(width, height) };

    let heap = roc::render_texture_heap();

    let alloc_result = heap.alloc_for(render_texture);
    match alloc_result {
        Ok(roc_box) => RocResult::ok(roc_box),
        Err(_) => {
            exit_with_msg("Unable to load render texture, out of memory in the render texture heap. Consider using ROC_RAY_MAX_RENDER_TEXTURE_HEAP_SIZE env var to increase the heap size.".into(), ExitErrCode::ExitHeapFull);
        }
    }
}

#[no_mangle]
extern "C" fn roc_fx_updateCamera(
    boxed_camera: RocBox<()>,
    target: &glue::RocVector2,
    offset: &glue::RocVector2,
    rotation: f32,
    zoom: f32,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::UpdateCamera) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let camera: &mut bindings::Camera2D =
        ThreadSafeRefcountedResourceHeap::box_to_resource(boxed_camera);

    camera.target = target.into();
    camera.offset = offset.into();
    camera.rotation = rotation;
    camera.zoom = zoom;

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_beginDrawing(clear_color: glue::RocColor) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::BeginDrawingFramebuffer) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        trace_log("BeginDrawing");

        bindings::BeginDrawing();
        bindings::ClearBackground(clear_color.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_endDrawing() -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::EndDrawingFramebuffer) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        trace_log("EndDrawing");
        bindings::EndMode2D();
    }

    RocResult::ok(())
}

#[allow(unused_variables)]
#[no_mangle]
extern "C" fn roc_fx_beginMode2D(boxed_camera: RocBox<()>) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::BeginMode2D) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        trace_log("BeginMode2D");

        let camera: &mut bindings::Camera2D =
            ThreadSafeRefcountedResourceHeap::box_to_resource(boxed_camera);

        bindings::BeginMode2D(*camera);
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_endMode2D(_boxed_camera: RocBox<()>) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::EndMode2D) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        trace_log("EndMode2D");
        bindings::EndMode2D();
    }

    RocResult::ok(())
}

#[allow(unused_variables)]
#[no_mangle]
extern "C" fn roc_fx_beginTexture(
    boxed_render_texture: RocBox<()>,
    clear_color: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::BeginDrawingTexture) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        trace_log("BeginTexture");
        let render_texture: &mut bindings::RenderTexture =
            ThreadSafeRefcountedResourceHeap::box_to_resource(boxed_render_texture);

        bindings::BeginTextureMode(*render_texture);
        bindings::ClearBackground(clear_color.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_endTexture(_boxed_render_texture: RocBox<()>) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::EndDrawingTexture) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    unsafe {
        trace_log("EndTexture");
        bindings::EndTextureMode();
    }

    RocResult::ok(())
}

fn get_mouse_button_states() -> RocList<u8> {
    let mouse_buttons: [u8; 7] = array::from_fn(|i| {
        unsafe {
            if bindings::IsMouseButtonPressed(i as c_int) {
                0
            } else if bindings::IsMouseButtonReleased(i as c_int) {
                1
            } else if bindings::IsMouseButtonDown(i as c_int) {
                2
            } else {
                // Up
                3
            }
        }
    });

    RocList::from_slice(&mouse_buttons)
}

fn get_keys_states() -> RocList<u8> {
    let keys: [u8; 350] = array::from_fn(|i| {
        unsafe {
            if bindings::IsKeyPressed(i as c_int) {
                0
            } else if bindings::IsKeyReleased(i as c_int) {
                1
            } else if bindings::IsKeyDown(i as c_int) {
                2
            } else if bindings::IsKeyUp(i as c_int) {
                3
            } else {
                // PressedRepeat
                4
            }
        }
    });

    RocList::from_slice(&keys)
}

#[no_mangle]
extern "C" fn roc_fx_loadSound(path: &RocStr) -> RocResult<RocBox<()>, ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::LoadSound) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let path = CString::new(path.as_str()).unwrap();

    let sound = unsafe {
        trace_log("LoadSound");
        bindings::LoadSound(path.as_ptr())
    };

    let heap = roc::sound_heap();

    let alloc_result = heap.alloc_for(sound);
    match alloc_result {
        Ok(roc_box) => RocResult::ok(roc_box),
        Err(_) => {
            exit_with_msg("Unable to load sound, out of memory in the sound heap. Consider using ROC_RAY_MAX_SOUNDS_HEAP_SIZE env var to increase the heap size.".into(), ExitErrCode::ExitHeapFull);
        }
    }
}

#[no_mangle]
extern "C" fn roc_fx_playSound(boxed_sound: RocBox<()>) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::PlaySound) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let sound: &mut bindings::Sound =
        ThreadSafeRefcountedResourceHeap::box_to_resource(boxed_sound);

    unsafe {
        bindings::PlaySound(*sound);
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_loadTexture(file_path: &RocStr) -> RocResult<RocBox<()>, ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::LoadTexture) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    // should have a valid utf8 string from roc, no need to check for null bytes
    let file_path = CString::new(file_path.as_str()).unwrap();

    let texture: bindings::Texture = unsafe { bindings::LoadTexture(file_path.as_ptr()) };

    let heap = roc::texture_heap();

    let alloc_result = heap.alloc_for(texture);
    match alloc_result {
        Ok(roc_box) => RocResult::ok(roc_box),
        Err(_) => {
            exit_with_msg("Unable to load texture, out of memory in the texture heap. Consider using ROC_RAY_MAX_TEXTURES_HEAP_SIZE env var to increase the heap size.".into(), ExitErrCode::ExitHeapFull);
        }
    }
}

#[no_mangle]
extern "C" fn roc_fx_drawTextureRec(
    boxed_texture: RocBox<()>,
    source: &glue::RocRectangle,
    position: &glue::RocVector2,
    color: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawTextureRectangle) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let texture: &mut bindings::Texture =
        ThreadSafeRefcountedResourceHeap::box_to_resource(boxed_texture);

    unsafe {
        bindings::DrawTextureRec(*texture, source.into(), position.into(), color.into());
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_drawRenderTextureRec(
    boxed_texture: RocBox<()>,
    source: &glue::RocRectangle,
    position: &glue::RocVector2,
    color: glue::RocColor,
) -> RocResult<(), ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::DrawTextureRectangle) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let texture: &mut bindings::RenderTexture =
        ThreadSafeRefcountedResourceHeap::box_to_resource(boxed_texture);

    unsafe {
        bindings::DrawTextureRec(
            texture.texture,
            source.into(),
            position.into(),
            color.into(),
        );
    }

    RocResult::ok(())
}

#[no_mangle]
extern "C" fn roc_fx_loadFileToStr(path: &RocStr) -> RocResult<RocStr, ()> {
    if let Err(msg) = platform_mode::update(PlatformEffect::LoadFileToStr) {
        exit_with_msg(msg, ExitErrCode::ExitEffectNotPermitted);
    }

    let path = path.as_str();
    let Ok(contents) = std::fs::read_to_string(path) else {
        panic!("file not found: {path}");
    };

    let contents = contents.replace("\r\n", "\n");
    let contents = unsafe { RocStr::from_slice_unchecked(contents.as_bytes()) };

    RocResult::ok(contents)
}

#[allow(unused_variables)]
fn trace_log(msg: &str) {
    #[cfg(feature = "trace-debug")]
    unsafe {
        let level = bindings::TraceLogLevel_LOG_DEBUG;
        let text = CString::new(msg).unwrap();
        bindings::TraceLog(level as i32, text.as_ptr());
    }
}
