use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet, fs, io::{BufRead, BufReader, Write},
    path::PathBuf, process::{Child, Command, Stdio},
    sync::{atomic::{AtomicBool, Ordering}, mpsc},
    thread, time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, String>;
const COLS: &[u8] = b"ABCDEFGHJKLMNOPQRST";
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Rule { Freestyle, Standard, Renju, Caro }
impl Rule {
    pub fn engine_rules(self) -> Value {
        match self {
            Self::Freestyle => json!({"basicrule":"FREESTYLE"}),
            Self::Standard => json!({"basicrule":"STANDARD"}),
            Self::Renju => json!({"basicrule":"RENJU"}),
            Self::Caro => json!({"sixwinrule":"SIXWINRULE_CARO","wallblock":false}),
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Color { B, W }
impl Color {
    pub fn other(self) -> Self { match self { Self::B => Self::W, Self::W => Self::B } }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct Point { pub x: usize, pub y: usize }
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Stone { pub x: usize, pub y: usize, pub color: Color }
impl Stone {
    pub fn point(self) -> Point { Point{x:self.x,y:self.y} }
}
pub fn check_size(size: usize) -> Result<()> {
    if matches!(size,15|19) { Ok(()) } else { Err("Only 15x15 and 19x19 are supported".into()) }
}
pub fn to_gtp(p: Point, n: usize) -> Result<String> {
    check_size(n)?;
    if p.x >= n || p.y >= n { return Err("Coordinate outside board".into()) }
    Ok(format!("{}{}", COLS[p.x] as char,n-p.y))
}
pub fn from_gtp(s: &str, n: usize) -> Result<Point> {
    check_size(n)?;
    let s = s.trim().to_ascii_uppercase();
    let b = s.as_bytes();
    let x = b.first().and_then(|a| COLS.iter().position(|c| c==a))
        .ok_or_else(||format!("Invalid coordinate: {s}"))?;
    let row: usize = s.get(1..).ok_or("Invalid coordinate")?.parse().map_err(|_| "Invalid row")?;
    if x >= n || row == 0 || row > n { return Err(format!("Coordinate outside board: {s}")) }
    Ok(Point{x,y:n-row})
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub rule: Rule,
    pub size: usize,
    pub executable: PathBuf,
    pub model: PathBuf,
    pub config: PathBuf,
    // Optional engine-specific rule parameters; the rule family cannot be replaced.
    #[serde(default)]
    pub extra_rules: serde_json::Map<String, Value>,
}
impl Profile {
    pub fn rules(&self) -> Result<Value> {
        let mut rules = self.rule.engine_rules();
        for (k,v) in &self.extra_rules {
            if !matches!(k.as_str(),"vcnrule"|"firstpasswin"|"maxmoves"|"wallblock") ||
                (k=="wallblock" && self.rule != Rule::Caro) {
                return Err(format!("Unsupported rule override: {k}"));
            }
            rules[k] = v.clone();
        }
        let vcn = rules.get("vcnrule").and_then(Value::as_str).unwrap_or("NOVC") != "NOVC";
        let fpw = rules.get("firstpasswin").and_then(Value::as_bool).unwrap_or(false);
        let mm = rules.get("maxmoves").and_then(Value::as_i64).unwrap_or(0)>0;
        if (vcn as u8 + fpw as u8 + mm as u8)>1 { return Err("VCN, first-pass-win and maxmoves are mutually exclusive".into()) }
        Ok(rules)
    }
    pub fn command(&self, mode: &str) -> Result<Command> {
        check_size(self.size)?;
        let abs = |p: &PathBuf| -> Result<PathBuf> {
            let p = fs::canonicalize(p).map_err(|e| format!("{}: {e}",p.display()))?;
            if !p.is_file(){return Err(format!("Not a file: {}",p.display()))}
            Ok(p)
        };
        let exe = abs(&self.executable)?;
        let model = abs(&self.model)?;
        let config = abs(&self.config)?;
        let mut cmd = Command::new(&exe);
        cmd.arg(mode).arg("-model").arg(model).arg("-config").arg(config);
        if let Some(dir)=exe.parent(){cmd.current_dir(dir);}
        Ok(cmd)
    }
}
pub fn profile_for(profiles: &[Profile], rule: Rule, size: usize) -> Result<Profile> {
    check_size(size)?;
    let mut found = profiles.iter().filter(|p|p.rule==rule&&p.size==size);
    let p=found.next().ok_or_else(||format!("No model profile for {rule:?} {size}x{size}"))?;
    if found.next().is_some(){return Err("Duplicate rule/size profile".into())}
    Ok(p.clone())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub size: usize,
    pub rule: Rule,
    pub stones: Vec<Stone>,
    pub next: Color,
    #[serde(default)]
    pub excluded: Vec<Point>,
    #[serde(default)] pub history_known: bool,
}
impl Position {
    pub fn new(size: usize, rule: Rule)->Self{Self{size,rule,stones:vec![],next:Color::B,excluded:vec![],history_known:true}}
    pub fn validate(&self)->Result<()>{
        check_size(self.size)?;
        let mut occupied=HashSet::new();
        for s in &self.stones {
            to_gtp(s.point(),self.size)?;
            if !occupied.insert(s.point()) {return Err("Duplicate stone".into())}
        }
        for &p in &self.excluded{to_gtp(p,self.size)?;}
        Ok(())
    }
    pub fn play(&mut self,p:Point)->Result<()>{
        to_gtp(p,self.size)?;
        if self.stones.iter().any(|s|s.point()==p){return Err("Occupied intersection".into())}
        self.stones.push(Stone{x:p.x,y:p.y,color:self.next});self.next=self.next.other();Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {pub visits:u64,pub seconds:f64}
impl Default for Limits {fn default()->Self{Self{visits:500,seconds:10.0}}}
pub fn make_query(p:&Profile,b:&Position,l:&Limits)->Result<Value>{
    b.validate()?;
    if p.rule!=b.rule||p.size!=b.size{return Err("Engine profile does not match rule and board".into())}
    if !(1..=10_000_000).contains(&l.visits)||!l.seconds.is_finite()||!(0.1..=600.0).contains(&l.seconds){return Err("Invalid visits/time limit".into())}
    // Gomocup BOARD may supply unordered stones. Initial placements preserve
    // the board without pretending that a sorted position is move history.
    let stones:Vec<_>=b.stones.iter().map(|s|Ok(json!([s.color,to_gtp(s.point(),b.size)?]))).collect::<Result<_>>()?;
    let (placements,moves,initial_player)=if b.history_known {(vec![],stones,b.stones.first().map(|s|s.color).unwrap_or(b.next))}else{(stones,vec![],b.next)};
    let mut q=json!({"id":"studio","boardXSize":b.size,"boardYSize":b.size,
        "rules":p.rules()?,"initialStones":placements,"moves":moves,
        "initialPlayer":initial_player,"maxVisits":l.visits,"analysisPVLen":16,
        "includePolicy":true,"includePVVisits":true,"reportDuringSearchEvery":0.25,
        "overrideSettings":{"reportAnalysisWinratesAs":"BLACK","maxTime":l.seconds}});
    if !b.excluded.is_empty(){
        let coords=b.excluded.iter().map(|&p|to_gtp(p,b.size)).collect::<Result<Vec<_>>>()?;
        q["avoidMoves"]=json!([{"player":b.next,"moves":coords,"untilDepth":1}]);
    }
    Ok(q)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    #[serde(rename="move")] pub coord:String,
    pub visits:u64, pub winrate:f64, pub drawrate:Option<f64>, pub prior:f64,
    #[serde(default)] pub order:u32,
    #[serde(default)] pub pv:Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct RootInfo {pub visits:u64,pub winrate:f64,pub current_player:Option<String>}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Analysis {
    pub root_info:RootInfo,
    pub move_infos:Vec<Candidate>,
    #[serde(default)] pub policy:Vec<f64>,
    #[serde(default)] pub is_during_search:bool,
    #[serde(default)] pub forbidden:Option<Vec<String>>,
}
impl Analysis {
    pub fn best(&self,b:&Position)->Option<Point>{
        self.move_infos.iter().filter_map(|c|{
            let p=from_gtp(&c.coord,b.size).ok()?;
            if b.excluded.contains(&p)||self.forbidden.as_ref().is_some_and(|f|f.contains(&c.coord))||b.stones.iter().any(|s|s.point()==p){None}else{Some((c.order,p))}
        }).min_by_key(|(order,_)|*order).map(|(_,p)|p)
    }
}
struct Managed(Child);
impl Drop for Managed {
    fn drop(&mut self){let _=self.0.kill();let _=self.0.wait();}
}
/// Owns one engine process for a single job, including multi-position searches.
/// Drop always terminates/reaps it. Requests are sequential with distinct IDs.
pub(crate) struct AnalysisSession {
    _child:Managed,
    input:std::process::ChildStdin,
    rx:mpsc::Receiver<String>,
    log:std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    sequence:u64,
}
impl AnalysisSession {
    pub(crate) fn start(profile:&Profile)->Result<Self>{
        let mut cmd=profile.command("analysis")?;
        let mut child=Managed(cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().map_err(|e|format!("Engine launch failed: {e}"))?);
        let stdout=child.0.stdout.take().ok_or("Missing stdout")?;
        let stderr=child.0.stderr.take().ok_or("Missing stderr")?;
        let (tx,rx)=mpsc::channel::<String>();
        thread::spawn(move||{for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok){if tx.send(line).is_err(){break}}});
        let log=std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
        let stderr_log=log.clone();
        thread::spawn(move||{for line in BufReader::new(stderr).lines().map_while(std::result::Result::ok){if let Ok(mut log)=stderr_log.lock(){if log.len()>=12{log.pop_front();}log.push_back(line);}}});
        let input=child.0.stdin.take().ok_or("Missing stdin")?;
        Ok(Self{_child:child,input,rx,log,sequence:0})
    }
    pub(crate) fn query<F:FnMut(&Analysis)>(&mut self,profile:&Profile,b:&Position,l:&Limits,cancel:&AtomicBool,mut progress:F)->Result<Analysis>{
        if cancel.load(Ordering::Relaxed){return Err("Search cancelled".into())}
        let mut q=make_query(profile,b,l)?;
        self.sequence+=1;
        let id=format!("studio-{}",self.sequence);
        q["id"]=json!(id);
        writeln!(self.input,"{q}").and_then(|_|self.input.flush()).map_err(|e|format!("Engine input: {e}"))?;
        // Only the first request pays the model initialization allowance.
        let startup=if self.sequence==1{180.0}else{3.0};
        let deadline=Instant::now()+Duration::from_secs_f64(l.seconds+startup);
        let mut last:Option<Analysis>=None;
        loop {
            if cancel.load(Ordering::Relaxed){return last.ok_or_else(||"Search cancelled before first result".into())}
            if Instant::now()>=deadline{return Err("Engine startup/search timed out".into())}
            match self.rx.recv_timeout(Duration::from_millis(75)) {
                Ok(line)=>{
                    let v:Value=match serde_json::from_str(&line){Ok(v)=>v,Err(_)=>continue};
                    if v.get("id").and_then(Value::as_str)!=Some(id.as_str()){continue}
                    if let Some(e)=v.get("error"){return Err(format!("KataGomo: {e}"))}
                    if let Some(w)=v.get("warning"){
                        if v.get("field").and_then(Value::as_str)==Some("rules")||w.to_string().contains("not supported by neural net"){
                            return Err(format!("Model/rule incompatibility: {w}"))
                        }
                        continue
                    }
                    if v.get("noResults").and_then(Value::as_bool)==Some(true){
                        return Err("Engine reported no results".into())
                    }
                    if v.get("rootInfo").is_none(){continue}
                    let a:Analysis=serde_json::from_value(v).map_err(|e|format!("Analysis response: {e}"))?;
                    progress(&a);
                    if !a.is_during_search{return Ok(a)}
                    last=Some(a);
                }
                Err(mpsc::RecvTimeoutError::Timeout)=>{},
                Err(mpsc::RecvTimeoutError::Disconnected)=>{
                    let details=self.log.lock().map(|q|q.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default();
                    return Err(format!("Engine exited before final analysis.\n{details}"))
                }
            }
        }
    }
}
pub fn analyze<F:FnMut(&Analysis)>(profile:&Profile,b:&Position,l:&Limits,cancel:&AtomicBool,progress:F)->Result<Analysis>{
    make_query(profile,b,l)?;
    AnalysisSession::start(profile)?.query(profile,b,l,cancel,progress)
}
pub mod special;
#[cfg(test)]
mod tests {
    use super::*;
    fn profile(rule:Rule,size:usize)->Profile{Profile{rule,size,executable:"engine".into(),model:"model".into(),config:"cfg".into(),extra_rules:Default::default()}}
    #[test]fn coords(){for n in [15,19]{for y in 0..n{for x in 0..n{let p=Point{x,y};assert_eq!(from_gtp(&to_gtp(p,n).unwrap(),n).unwrap(),p)}}}assert!(from_gtp("I8",15).is_err());assert_eq!(to_gtp(Point{x:18,y:18},19).unwrap(),"T1");}
    #[test]fn profiles_are_exact(){let a=[profile(Rule::Renju,15)];assert!(profile_for(&a,Rule::Renju,19).is_err());assert!(profile_for(&a,Rule::Standard,15).is_err());}
    #[test]fn query_uses_rules_and_color(){let mut b=Position::new(19,Rule::Renju);b.play(Point{x:9,y:9}).unwrap();let q=make_query(&profile(Rule::Renju,19),&b,&Limits::default()).unwrap();assert_eq!(q["rules"]["basicrule"],"RENJU");assert_eq!(q["moves"][0][1],"K10");assert_eq!(q["initialPlayer"],"B");assert_eq!(q["boardXSize"],19);}
    #[test]fn occupied_is_rejected(){let mut b=Position::new(15,Rule::Standard);let p=Point{x:0,y:0};b.play(p).unwrap();assert!(b.play(p).is_err());}
    #[test]fn caro_is_separate(){assert_eq!(Rule::Caro.engine_rules()["sixwinrule"],"SIXWINRULE_CARO");}
    #[test]fn invalid_rule_override(){let mut p=profile(Rule::Renju,15);p.extra_rules.insert("basicrule".into(),json!("STANDARD"));assert!(p.rules().is_err());}
}
