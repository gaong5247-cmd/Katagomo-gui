#![cfg(unix)]
// Tests the full CLI command -> JSON engine session -> coordinate response path.
// The fixture's estimates are deterministic, not real neural inference.
use std::{fs,io::{BufRead,BufReader,Write},path::PathBuf,process::{Child,ChildStdin,Command,Stdio},sync::mpsc,thread,time::{Duration,SystemTime,UNIX_EPOCH}};
use std::os::unix::fs::PermissionsExt;
use serde_json::json;

struct Harness{
    dir:PathBuf,
    child:Child,
    input:ChildStdin,
    rx:mpsc::Receiver<String>,
}
impl Harness{
    fn new()->Self{
        let suffix=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir=std::env::temp_dir().join(format!("kata-bridge-{}-{suffix}",std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe=dir.join("fixture.py");
        fs::write(&exe,include_str!("fixtures/analysis_fixture.py")).unwrap();
        fs::set_permissions(&exe,fs::Permissions::from_mode(0o755)).unwrap();
        let model=dir.join("fake-model");let cfg=dir.join("fake-config");
        fs::write(&model,b"fixture").unwrap();fs::write(&cfg,b"fixture").unwrap();
        let profile=dir.join("profiles.json");
        fs::write(&profile,serde_json::to_vec(&json!([{"rule":"freestyle","size":15,"executable":exe,"model":model,"config":cfg}])).unwrap()).unwrap();
        let mut child=Command::new(env!("CARGO_BIN_EXE_katagomo-bridge")).arg(profile)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
        let input=child.stdin.take().unwrap();let output=child.stdout.take().unwrap();
        let (tx,rx)=mpsc::channel();
        thread::spawn(move||{for line in BufReader::new(output).lines().map_while(std::result::Result::ok){if tx.send(line).is_err(){break}}});
        Self{dir,child,input,rx}
    }
    fn send(&mut self,line:&str){writeln!(self.input,"{line}").unwrap();self.input.flush().unwrap();}
    fn response(&self)->String{
        loop{
            let line=self.rx.recv_timeout(Duration::from_secs(15)).expect("bridge response timeout");
            assert!(!line.starts_with("ERROR"),"{line}");
            if !line.starts_with("MESSAGE"){return line}
        }
    }
    fn start(&mut self){
        self.send("START 15");assert_eq!(self.response(),"OK");
        self.send("INFO MAX_NODE 120");self.send("INFO TIMEOUT_TURN 1000");
    }
}
impl Drop for Harness{
    fn drop(&mut self){let _=self.child.kill();let _=self.child.wait();let _=fs::remove_dir_all(&self.dir);}
}
#[test]fn special_protocol_commands(){
    let mut h=Harness::new();
    h.start();
    h.send("YXBALANCEONE 0");assert_eq!(h.response(),"1,0");
    h.send("TAKEBACK 1,0");assert_eq!(h.response(),"OK");
    h.send("YXBALANCETWO 0");assert_eq!(h.response(),"1,0 2,0");
    h.send("TAKEBACK 2,0");assert_eq!(h.response(),"OK");
    h.send("TAKEBACK 1,0");assert_eq!(h.response(),"OK");
    h.send("RESTART");assert_eq!(h.response(),"OK");
    h.send("YXSEARCHDEFEND");assert_eq!(h.response(),"0,0");
    h.send("END");
}
