//! Local Gomocup/Yixin adapter. Never emits invented alpha-beta search scores.
use katagomo_core::*;
use katagomo_core::special::{search_special,SpecialMode,SpecialResult};
use std::{fs,io::{self,BufRead,Write},process::Stdio,sync::{Arc,atomic::{AtomicBool,Ordering},mpsc},thread,time::{Duration,Instant}};
fn load_profiles(path:&str)->Result<Vec<Profile>>{
    let s=fs::read_to_string(path).map_err(|e|format!("{path}: {e}"))?;
    serde_json::from_str(&s).map_err(|e|format!("Profile JSON: {e}"))
}
fn parse_rule(s:&str)->Result<Rule>{match s.to_ascii_lowercase().as_str(){"freestyle"=>Ok(Rule::Freestyle),"standard"=>Ok(Rule::Standard),"renju"=>Ok(Rule::Renju),"caro"=>Ok(Rule::Caro),_=>Err("Unknown rule".into())}}
fn xy(s:&str,n:usize)->Result<Point>{
    let (x,y)=s.trim().split_once(',').ok_or("Expected x,y")?;
    let p=Point{x:x.parse().map_err(|_|"Invalid x")?,y:y.parse().map_err(|_|"Invalid y")?};
    to_gtp(p,n)?;Ok(p)
}
fn print_line(s:impl std::fmt::Display){println!("{s}");let _=io::stdout().flush();}
fn raw_mode(p:&Profile,mode:&str)->Result<()> {
    let status=p.command(mode)?.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
      .status().map_err(|e|e.to_string())?;
    if status.success(){Ok(())}else{Err(format!("Engine exited: {status}"))}
}
#[derive(Clone,Copy)]enum Kind{Move(usize),Forbid,Bench,Special(SpecialMode,i32)}
enum Outcome{Analysis(Analysis),Special(SpecialResult)}
struct Job{cancel:Arc<AtomicBool>,result:mpsc::Receiver<Result<Outcome>>,kind:Kind,start:Instant}
struct Adapter{
    path:String,profiles:Vec<Profile>,board:Option<Position>,rule:Rule,limits:Limits,detail:bool,job:Option<Job>,
    self_color:Option<Color>,timeout_turn:f64,timeout_match:f64,time_left:f64,
}
impl Adapter{
    fn new(path:String,profiles:Vec<Profile>)->Self{Self{path,profiles,board:None,rule:Rule::Freestyle,limits:Limits::default(),detail:false,job:None,self_color:None,timeout_turn:10.0,timeout_match:600.0,time_left:600.0}}
    fn board(&self)->Result<&Position>{self.board.as_ref().ok_or_else(||"START required".into())}
    fn board_mut(&mut self)->Result<&mut Position>{self.board.as_mut().ok_or_else(||"START required".into())}
    fn start(&mut self,n:usize)->Result<()>{check_size(n)?;self.board=Some(Position::new(n,self.rule));self.self_color=None;Ok(())}
    fn launch(&mut self,kind:Kind)->Result<()>{
        let b=self.board()?.clone();let p=profile_for(&self.profiles,b.rule,b.size)?;
        let mut limits=self.limits.clone();limits.seconds=self.timeout_turn.min(self.timeout_match).min(self.time_left).clamp(0.1,600.0);
        if matches!(kind,Kind::Forbid){limits.visits=1;limits.seconds=0.1}
        let detail=self.detail;let cancel=Arc::new(AtomicBool::new(false));let flag=cancel.clone();
        let (tx,rx)=mpsc::channel();
        let start=Instant::now();
        thread::spawn(move||{
            let result=if let Kind::Special(mode,bias)=kind {
                search_special(&p,&b,&limits,mode,bias,&flag,|a|{
                    if detail {print_line(format!("MESSAGE root visits={} black_eval={:.4}",a.root_info.visits,a.root_info.winrate))}
                }).map(Outcome::Special)
            }else{analyze(&p,&b,&limits,&flag,|a|{
                if detail&&matches!(kind,Kind::Move(_)){
                    print_line(format!("MESSAGE visits={} black_eval={:.4}",a.root_info.visits,a.root_info.winrate));
                }
            }).map(Outcome::Analysis)};
            let _=tx.send(result);
        });
        self.job=Some(Job{cancel,result:rx,kind,start});Ok(())
    }
    fn poll(&mut self){
        let ready=self.job.as_ref().and_then(|j|match j.result.try_recv(){Ok(r)=>Some(r),Err(mpsc::TryRecvError::Disconnected)=>Some(Err("Search worker exited".into())),Err(mpsc::TryRecvError::Empty)=>None});
        if let Some(r)=ready{
            let j=self.job.take().unwrap();
            match r{
                Err(e)=>print_line(format!("ERROR {}",e.replace('\n'," "))),
                Ok(Outcome::Special(r))=>{
                    let b=self.board.as_mut().unwrap();
                    let label=match r.mode{SpecialMode::BalanceOne=>"YXBALANCEONE",SpecialMode::BalanceTwo=>"YXBALANCETWO",SpecialMode::Defend=>"YXSEARCHDEFEND"};
                    print_line(format!("MESSAGE {label} branches={} visits={} target_black={} interrupted={}",r.evaluated_branches,r.total_visits,r.target_black.map(|x|format!("{x:.6}")).unwrap_or_else(||"n/a".into()),r.interrupted));
                    let n=r.lines.len();
                    for (i,l) in r.lines.iter().enumerate(){
                        if r.mode!=SpecialMode::Defend&&!self.detail&&i>0{break}
                        let moves=l.points.iter().map(|p|format!("{},{}",p.x,p.y)).collect::<Vec<_>>().join(" ");
                        print_line(format!("MESSAGE INFO PV {i}"));
                        print_line(format!("MESSAGE INFO NUMPV {n}"));
                        print_line(format!("MESSAGE INFO WINRATE {:.6}",if b.next==Color::B{l.black_eval}else{1.0-l.black_eval}));
                        print_line(format!("MESSAGE INFO BESTLINE {moves}"));
                        print_line("MESSAGE INFO PV DONE");
                        print_line(format!("MESSAGE {label} line={moves} black_eval={:.6} visits={}",l.black_eval,l.visits));
                    }
                    if let Some(l)=r.lines.first(){
                        let mut copy=b.clone();
                        let result=l.points.iter().try_for_each(|p|copy.play(*p));
                        match result{
                            Ok(())=>{*b=copy;print_line(l.points.iter().map(|p|format!("{},{}",p.x,p.y)).collect::<Vec<_>>().join(" "))},
                            Err(e)=>print_line(format!("ERROR {e}")),
                        }
                    }else{print_line("ERROR no evaluated line")}
                }
                Ok(Outcome::Analysis(a))=>match j.kind{
                    Kind::Move(n)=>{
                        let b=self.board.as_mut().unwrap();
                        let mut candidates=a.move_infos.clone();candidates.sort_by_key(|c|c.order);
                        if self.detail||n>1{for c in candidates.iter().take(n.max(1)){
                            if let Ok(p)=from_gtp(&c.coord,b.size){print_line(format!("MESSAGE move={},{} visits={} black_eval={:.4} pv={}",p.x,p.y,c.visits,c.winrate,c.pv.join(" ")))}
                        }}
                        if let Some(p)=a.best(b){
                            match b.play(p){Ok(())=>print_line(format!("{},{}",p.x,p.y)),Err(e)=>print_line(format!("ERROR {e}"))}
                        }else{print_line("ERROR no valid candidate")}
                    }
                    Kind::Forbid=>{
                        if let Some(points)=a.forbidden{
                            let n=self.board.as_ref().unwrap().size;
                            let s=points.iter().filter_map(|p|from_gtp(p,n).ok()).map(|p|format!("{:02}{:02}",p.x,p.y)).collect::<String>();
                            print_line(format!("FORBID {s}."));
                        }else{print_line("ERROR forbidden-point output requires the patched Gom2024 engine")}
                    }
                    Kind::Bench=>print_line(format!("MESSAGE visits={} wall_seconds={:.3} (includes process/model startup)",a.root_info.visits,j.start.elapsed().as_secs_f64())),
                    Kind::Special(..)=>print_line("ERROR invalid search response"),
                }
            }
        }
    }
    fn stop(&self){if let Some(j)=&self.job{j.cancel.store(true,Ordering::Relaxed)}}
    fn set_position(&mut self,rows:Vec<String>)->Result<()>{
        let n=self.board()?.size;
        let mut relative=Vec::new();
        for row in rows{
            let (loc,c)=row.rsplit_once(',').ok_or("Expected x,y,field")?;
            let point=xy(loc,n)?;
            let side:u8=c.parse().map_err(|_|"Invalid field")?;
            if !matches!(side,1|2){return Err("Only self=1 and opponent=2 supported; walls unsupported".into())}
            relative.push((point,side));
        }
        let mine=relative.iter().filter(|(_,c)|*c==1).count();let opp=relative.len()-mine;
        let own=if mine==opp{Color::B}else if opp==mine+1{Color::W}else{return Err("BOARD is not a valid alternating position for the engine to move".into())};
        let mut b=Position::new(n,self.rule);b.history_known=false;b.next=own;
        b.stones=relative.into_iter().map(|(p,side)|Stone{x:p.x,y:p.y,color:if side==1{own}else{own.other()}}).collect();
        b.validate()?;self.self_color=Some(own);self.board=Some(b);Ok(())
    }
    fn info(&mut self,arg:&str)->Result<()>{
        let (key,value)=arg.split_once(char::is_whitespace).ok_or("INFO requires key and value")?;
        let val=value.trim();
        match key.to_ascii_uppercase().as_str(){
            "RULE"=>{
                let r=match val{"0"=>Rule::Freestyle,"1"=>Rule::Standard,"2"|"4"=>Rule::Renju,_=>return Err("Unsupported Gomocup rule ID/opening rule".into())};
                self.rule=r;if let Some(b)=&self.board{self.start(b.size)?}
            }
            "TIMEOUT_TURN"=>self.timeout_turn=ms(val)?,
            "TIMEOUT_MATCH"=>self.timeout_match=ms(val)?,
            "TIME_LEFT"=>self.time_left=ms(val)?,
            "MAX_NODE"=>{let n:u64=val.parse().map_err(|_|"Invalid max node")?;self.limits.visits=if n==0{10_000_000}else{n.min(10_000_000)};},
            "SHOW_DETAIL"=>self.detail=val!="0",
            // These are informational in the Gomocup protocol.
            "GAME_TYPE"|"FOLDER"=>{},
            _=>return Err(format!("Unsupported INFO {key}; engine threads/cache are configured in the profile cfg")),
        }Ok(())
    }
    fn run(&mut self,line:&str,rx:&mpsc::Receiver<String>)->Result<bool>{
        let (cmd,arg)=line.trim().split_once(char::is_whitespace).unwrap_or((line.trim(),""));
        let arg=arg.trim();let cmd=cmd.to_ascii_uppercase();
        if cmd=="END"{self.stop();return Ok(true)}
        if cmd=="STOP"||cmd=="YXSTOP"{self.stop();return Ok(false)}
        if self.job.is_some(){return Err("Engine busy; STOP first".into())}
        match cmd.as_str(){
            ""=>{},
            "ABOUT"=>print_line("name=\"KataGomo Bridge\", version=\"0.1.0\", author=\"KataGomo upstream / Katagomo-gui\", country=\"KR\""),
            "START"=>{self.start(arg.parse().map_err(|_|"Invalid board size")?)?;print_line("OK")},
            "RECTSTART"=>{let (x,y)=arg.split_once(',').ok_or("RECTSTART requires x,y")?;if x!=y{return Err("Only square boards supported".into())}self.start(x.parse().map_err(|_|"Invalid board size")?)?;print_line("OK")},
            "RESTART"=>{let n=self.board()?.size;self.start(n)?;print_line("OK")},
            "INFO"=>self.info(arg)?,
            "BEGIN"=>{if !self.board()?.stones.is_empty(){return Err("BEGIN requires empty board".into())}self.self_color=Some(Color::B);self.launch(Kind::Move(1))?},
            "TURN"=>{
                let b=self.board_mut()?;let p=xy(arg,b.size)?;b.play(p)?;
                let next=b.next;if let Some(c)=self.self_color{if c!=next{return Err("TURN side mismatch".into())}}else{self.self_color=Some(next)}
                self.launch(Kind::Move(1))?
            }
            "BOARD"|"YXBOARD"=>{let rows=block(rx,arg)?;self.set_position(rows)?;if cmd=="BOARD"{self.launch(Kind::Move(1))?}},
            "TAKEBACK"=>{
                let b=self.board_mut()?;let p=xy(arg,b.size)?;
                if let Some(i)=b.stones.iter().position(|s|s.point()==p){
                    if i+1!=b.stones.len(){b.history_known=false;}
                    b.next=b.stones.remove(i).color;print_line("OK")
                }else{print_line("UNKNOWN")}
            }
            "YXNBEST"=>{let n:usize=arg.parse().map_err(|_|"Invalid candidate count")?;self.launch(Kind::Move(n.clamp(1,361)))?},
            "YXBLOCK"|"YXBLOCKUNDO"=>{
                let rows=block(rx,arg)?;let b=self.board_mut()?;
                let points=rows.iter().map(|s|xy(s,b.size)).collect::<Result<Vec<_>>>()?;
                for p in points{if cmd=="YXBLOCK"{if !b.excluded.contains(&p){b.excluded.push(p)}}else{b.excluded.retain(|q|*q!=p)}}
            }
            "YXBLOCKRESET"=>self.board_mut()?.excluded.clear(),
            "YXSHOWFORBID"=>{
                let b=self.board()?;
                if b.rule!=Rule::Renju||b.next!=Color::B{print_line("FORBID .")}else{self.launch(Kind::Forbid)?}
            }
            "YXBALANCEONE"|"YXBALANCETWO"=>{
                let bias:i32=if arg.is_empty(){0}else{arg.parse().map_err(|_|"Invalid balance bias")?};
                katagomo_core::special::target_black(self.board()?.next,bias)?;
                let mode=if cmd=="YXBALANCEONE"{SpecialMode::BalanceOne}else{SpecialMode::BalanceTwo};
                self.launch(Kind::Special(mode,bias))?
            }
            "YXSEARCHDEFEND"=>self.launch(Kind::Special(SpecialMode::Defend,0))?,
            "YXSHOWINFO"=>{self.detail=true;print_line("MESSAGE KataGomo MCTS; coordinates are zero-based x,y")},
            "YXHASHCLEAR"=>{/* Every request starts an isolated engine with a fresh cache. */},
            "RELOADCONFIG"=>{self.profiles=load_profiles(&self.path)?;print_line("MESSAGE profiles/config reloaded")},
            "LOADMODEL"=>{
                let (r,n)={let b=self.board()?;(b.rule,b.size)};
                let p=self.profiles.iter_mut().find(|p|p.rule==r&&p.size==n).ok_or("No active profile")?;
                let path=std::path::PathBuf::from(arg.trim_matches('"'));
                if !path.is_file(){return Err("Model file not found".into())}p.model=path;print_line("MESSAGE model changed for active rule/size")
            }
            "TRACEBOARD"=>{let b=self.board()?;for y in 0..b.size{let row=(0..b.size).map(|x|b.stones.iter().find(|s|s.x==x&&s.y==y).map(|s|if s.color==Color::B{'X'}else{'O'}).unwrap_or('.')).collect::<String>();print_line(format!("MESSAGE {row}"))}},
            "BENCH"=>self.launch(Kind::Bench)?,
            _=>print_line(format!("UNKNOWN {cmd}")),
        }Ok(false)
    }
}
fn ms(s:&str)->Result<f64>{let v:u64=s.parse().map_err(|_|"Invalid milliseconds")?;Ok(if v==0{600.0}else{(v as f64/1000.0).clamp(0.1,600.0)})}
fn block(rx:&mpsc::Receiver<String>,initial:&str)->Result<Vec<String>>{
    let mut v=Vec::new();let mut line=initial.to_owned();
    loop{for token in line.split_whitespace(){if token.eq_ignore_ascii_case("DONE"){return Ok(v)}v.push(token.to_owned());if v.len()>1024{return Err("Too many block entries".into())}}
        line=rx.recv().map_err(|_|"EOF before DONE")?;
    }
}
fn main()->std::result::Result<(),String>{
    let args:Vec<String>=std::env::args().collect();
    if args.len()<2{return Err("Usage: katagomo-bridge profiles.json [--gtp|--json RULE SIZE]".into())}
    let profiles=load_profiles(&args[1])?;
    if args.len()>2{
        if args.len()!=5{return Err("Expected mode RULE SIZE".into())}
        let rule=parse_rule(&args[3])?;let size=args[4].parse().map_err(|_|"Invalid size")?;
        let p=profile_for(&profiles,rule,size)?;
        return raw_mode(&p,match args[2].as_str(){"--gtp"=>"gtp","--json"=>"analysis",_=>return Err("Unknown mode".into())})
    }
    let (tx,rx)=mpsc::channel::<String>();
    thread::spawn(move||for l in io::stdin().lock().lines().map_while(std::result::Result::ok){if tx.send(l).is_err(){break}});
    let mut a=Adapter::new(args[1].clone(),profiles);
    loop{a.poll();match rx.recv_timeout(Duration::from_millis(25)){
        Ok(l)=>match a.run(&l,&rx){Ok(true)=>break,Ok(false)=>{},Err(e)=>print_line(format!("ERROR {e}"))},
        Err(mpsc::RecvTimeoutError::Timeout)=>{},
        Err(mpsc::RecvTimeoutError::Disconnected)=>break,
    }}
    a.stop();if let Some(j)=a.job.take(){let _=j.result.recv_timeout(Duration::from_secs(3));}
    Ok(())
}
