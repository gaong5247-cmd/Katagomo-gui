#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use katagomo_core::{self as core, Analysis, Limits, Position, Profile};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use tauri::{Emitter, State};

#[derive(Default)]
struct Running(Mutex<Option<Arc<AtomicBool>>>);
#[tauri::command]
async fn analyze(app:tauri::AppHandle,state:State<'_,Running>,profiles:Vec<Profile>,position:Position,limits:Limits)->Result<Analysis,String>{
    let profile=core::profile_for(&profiles,position.rule,position.size)?;
    let cancel=Arc::new(AtomicBool::new(false));
    {
        let mut active=state.0.lock().map_err(|_|"Search lock poisoned")?;
        if active.is_some(){return Err("Search already active".into())}
        *active=Some(cancel.clone());
    }
    let result=tauri::async_runtime::spawn_blocking(move||{
        core::analyze(&profile,&position,&limits,&cancel,|a|{let _=app.emit("analysis-progress",a);})
    }).await;
    *state.0.lock().map_err(|_|"Search lock poisoned")?=None;
    result.map_err(|e|e.to_string())?
}
#[tauri::command]
fn stop(state:State<'_,Running>)->Result<(),String>{
    if let Some(flag)=state.0.lock().map_err(|_|"Search lock poisoned")?.as_ref(){flag.store(true,Ordering::Relaxed)}
    Ok(())
}
#[tauri::command]
fn save_sgf(path:String,position:Position)->Result<(),String>{
    position.validate()?;
    let mut sgf=format!("(;GM[4]FF[4]CA[UTF-8]SZ[{}]RU[{:?}]",position.size,position.rule);
    for s in position.stones {
        let col=(b'a'+s.x as u8) as char;let row=(b'a'+s.y as u8) as char;
        sgf.push_str(&format!(";{:?}[{}{}]",s.color,col,row));
    }
    sgf.push(')');
    std::fs::write(path,sgf).map_err(|e|e.to_string())
}
fn main(){
    tauri::Builder::default().manage(Running::default()).plugin(tauri_plugin_dialog::init())
      .invoke_handler(tauri::generate_handler![analyze,stop,save_sgf])
      .run(tauri::generate_context!()).expect("error while running KataGomo Studio");
}
