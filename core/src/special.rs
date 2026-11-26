//! Bounded neural opening balance and defensive MultiPV-style analysis.
//! Uses actual KataGomo estimates (black expected result, including half draws).
//! No claim of exact Rapfi alpha-beta values or exhaustive coverage.
use super::*;
use std::cmp::Ordering as Cmp;

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum SpecialMode { BalanceOne, BalanceTwo, Defend }

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Line {
    pub points:Vec<Point>,
    pub black_eval:f64,
    pub visits:u64,
    pub pv:Vec<String>,
}
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct SpecialResult {
    pub mode:SpecialMode,
    pub target_black:Option<f64>,
    pub lines:Vec<Line>,
    pub evaluated_branches:usize,
    pub total_visits:u64,
    pub interrupted:bool,
}
/// Rapfi bias is a signed evaluation from the original mover's perspective.
/// Use a logistic conversion with Rapfi's default scale (200). This is an
/// approximation in KataGomo expected-result space, not identical calibration.
pub fn target_black(side:Color,bias:i32)->Result<f64>{
    if !(-6000..=6000).contains(&bias){return Err("Balance bias must be in -6000..6000".into())}
    let own=1.0/(1.0+(-(bias as f64)/200.0).exp());
    Ok(if side==Color::B{own}else{1.0-own})
}
fn score(line:&Line,side:Color,target:Option<f64>)->f64{
    if let Some(t)=target{(line.black_eval-t).abs()}
    else if side==Color::B{1.0-line.black_eval}
    else{line.black_eval}
}
fn rank(lines:&mut [Line],side:Color,target:Option<f64>){
    lines.sort_by(|a,b|score(a,side,target).total_cmp(&score(b,side,target))
        .then_with(||b.visits.cmp(&a.visits))
        .then_with(||{
            for (x,y) in a.points.iter().zip(&b.points){
                let c=(x.y,x.x).cmp(&(y.y,y.x));if c!=Cmp::Equal{return c}
            }
            a.points.len().cmp(&b.points.len())
        }));
}
fn legal(p:Point,b:&Position,a:&Analysis)->bool{
    if p.x>=b.size||p.y>=b.size||b.excluded.contains(&p)||b.stones.iter().any(|s|s.point()==p){return false}
    let index=p.y*b.size+p.x;
    if a.policy.get(index).is_some_and(|p|!p.is_finite()||*p<0.0){return false}
    if b.rule==Rule::Renju&&b.next==Color::B{
        let Ok(c)=to_gtp(p,b.size)else{return false};
        if a.forbidden.as_ref().is_none_or(|f|f.contains(&c)){return false}
    }
    true
}
fn require_forbids(b:&Position,a:&Analysis)->Result<()>{
    if b.rule==Rule::Renju&&b.next==Color::B&&a.forbidden.is_none(){
        return Err("Special search on black Renju requires the patched forbidden-point engine".into())
    }
    Ok(())
}
fn expected(p:f64)->bool{p.is_finite()&&(0.0..=1.0).contains(&p)}
fn explored(b:&Position,a:&Analysis)->Vec<Line>{
    let mut seen=HashSet::new();
    a.move_infos.iter().filter_map(|c|{
        let p=from_gtp(&c.coord,b.size).ok()?;
        if c.visits==0||!expected(c.winrate)||!legal(p,b,a)||!seen.insert(p){return None}
        Some(Line{points:vec![p],black_eval:c.winrate,visits:c.visits,pv:c.pv.clone()})
    }).collect()
}
/// Mix explored moves close to the target, heavily visited moves, and raw
/// legal policy candidates. This prevents choosing only the normal best move.
fn shortlist(b:&Position,a:&Analysis,mode:SpecialMode,target:Option<f64>,cap:usize)->Vec<Point>{
    let mut seen=HashSet::new();let mut chosen=Vec::new();
    let mut add=|p:Point|{if chosen.len()<cap&&legal(p,b,a)&&seen.insert(p){chosen.push(p)}};
    let mut lines=explored(b,a);rank(&mut lines,b.next,target);
    for l in lines.iter().take((cap+1)/2){add(l.points[0]);}
    lines.sort_by(|a,b|b.visits.cmp(&a.visits));
    for l in lines.iter().take((cap+1)/3){add(l.points[0]);}
    let mut policy:Vec<_>=(0..b.size*b.size).filter_map(|i|{
        let p=Point{x:i%b.size,y:i/b.size};let v=*a.policy.get(i)?;
        if legal(p,b,a)&&v>0.0{Some((p,v))}else{None}
    }).collect();
    policy.sort_by(|a,b|b.1.total_cmp(&a.1));
    for (p,_) in policy{add(p);}
    if mode!=SpecialMode::BalanceTwo {for l in lines{add(l.points[0]);}}
    chosen
}
/// One job owns one initialized engine; child positions reuse the loaded model.
/// Root exclusions apply only to the first ply. The total requested visit/time
/// budget is divided over the root and child queries (startup is extra).
pub fn search_special<F:FnMut(&Analysis)>(
    profile:&Profile,b:&Position,limits:&Limits,mode:SpecialMode,bias:i32,cancel:&AtomicBool,mut progress:F
)->Result<SpecialResult>{
    make_query(profile,b,limits)?;
    let target=if mode==SpecialMode::Defend{None}else{Some(target_black(b.next,bias)?)};
    let mut session=AnalysisSession::start(profile)?;
    let cap=match mode{SpecialMode::Defend=>12,_=>8};
    // Reserve half of the requested work for initial MCTS. With very short
    // limits, perform only that search instead of silently exceeding the budget.
    let root_limits=if limits.visits>=4&&limits.seconds>=0.2{
        Limits{visits:(limits.visits/2).max(1),seconds:(limits.seconds*0.5).max(0.1)}
    }else{limits.clone()};
    let root=session.query(profile,b,&root_limits,cancel,&mut progress)?;
    require_forbids(b,&root)?;
    let mut total=root.root_info.visits;
    let mut lines=if mode==SpecialMode::BalanceTwo{Vec::new()}else{explored(b,&root)};
    let mut branches=0usize;
    let available_visits=limits.visits.saturating_sub(root_limits.visits);
    let available_seconds=(limits.seconds-root_limits.seconds).max(0.0);
    let capacity=cap.min(available_visits as usize).min((available_seconds/0.1+1e-8).floor() as usize);
    let mut candidates=shortlist(b,&root,mode,target,capacity);
    // A root analysis can report PVs even when there is no budget for a child
    // query, but a second-ply estimate cannot be inferred from a first-ply score.
    if mode==SpecialMode::BalanceTwo&&candidates.is_empty(){
        return Err("Two-ply balance needs more search time/visits and legal candidates".into())
    }
    let count=candidates.len();
    let child_limits=Limits{
        visits:(available_visits/(count.max(1) as u64)).max(1),
        seconds:(available_seconds/(count.max(1) as f64)).max(0.1),
    };
    for p in candidates.drain(..){
        if cancel.load(Ordering::Relaxed)||root.is_during_search{break}
        let mut child=b.clone();child.play(p)?;child.excluded.clear();
        let a=session.query(profile,&child,&child_limits,cancel,|_|{});
        let a=match a{
            Ok(a)=>a,
            Err(_) if cancel.load(Ordering::Relaxed)=>break,
            Err(e) if e=="Engine reported no results"=>continue, // terminal child, no estimate
            Err(e)=>return Err(e),
        };
        total=total.saturating_add(a.root_info.visits);
        if a.is_during_search||cancel.load(Ordering::Relaxed){break}
        branches+=1;
        if mode==SpecialMode::BalanceTwo{
            require_forbids(&child,&a)?;
            for mut l in explored(&child,&a){
                l.points.insert(0,p);l.pv.insert(0,to_gtp(p,b.size)?);lines.push(l)
            }
        }else if expected(a.root_info.winrate){
            // Child root evaluation is independent of the original root move
            // selection. It supersedes, rather than duplicates, that candidate.
            lines.retain(|l|l.points[0]!=p);
            let mut pv=vec![to_gtp(p,b.size)?];
            if let Some(best)=a.move_infos.iter().filter(|c|c.visits>0).min_by_key(|c|c.order){pv.extend(best.pv.clone());}
            lines.push(Line{points:vec![p],black_eval:a.root_info.winrate,visits:a.root_info.visits,pv});
        }
    }
    rank(&mut lines,b.next,target);
    if lines.is_empty(){return Err(if cancel.load(Ordering::Relaxed){"Cancelled before any valid line"}else{"No evaluated legal line found"}.into())}
    Ok(SpecialResult{mode,target_black:target,lines,evaluated_branches:branches,total_visits:total,interrupted:cancel.load(Ordering::Relaxed)||root.is_during_search})
}

#[cfg(test)]
mod tests{
    use super::*;
    fn l(x:usize,wr:f64,n:u64)->Line{Line{points:vec![Point{x,y:0}],black_eval:wr,visits:n,pv:vec![]}}
    #[test]fn balance_not_best_win(){let mut a=vec![l(0,0.9,100),l(1,0.49,2),l(2,0.60,10)];rank(&mut a,Color::B,Some(0.5));assert_eq!(a[0].points[0].x,1);}
    #[test]fn bias_changes_target(){assert_eq!(target_black(Color::B,0).unwrap(),0.5);let b=target_black(Color::B,200).unwrap();assert!(b>0.7&&b<0.75);assert!((b+target_black(Color::W,200).unwrap()-1.0).abs()<1e-9);assert!(target_black(Color::B,6001).is_err());}
    #[test]fn defense_uses_mover(){let mut a=vec![l(0,0.8,5),l(1,0.2,6)];rank(&mut a,Color::W,None);assert_eq!(a[0].points[0].x,1);rank(&mut a,Color::B,None);assert_eq!(a[0].points[0].x,0);}
    #[test]fn equal_scores_prefer_more_visits(){let mut a=vec![l(0,0.5,5),l(1,0.5,10)];rank(&mut a,Color::B,Some(0.5));assert_eq!(a[0].points[0].x,1);}
    #[test]fn pair_ranking_uses_second_ply_eval(){let mut a=vec![Line{points:vec![Point{x:0,y:0},Point{x:0,y:1}],black_eval:0.9,visits:100,pv:vec![]},Line{points:vec![Point{x:1,y:0},Point{x:1,y:1}],black_eval:0.51,visits:10,pv:vec![]}];rank(&mut a,Color::B,Some(0.5));assert_eq!(a[0].points.len(),2);assert_eq!(a[0].points[0].x,1);}
    #[test]fn excluded_and_forbidden_are_filtered(){
        let mut b=Position::new(15,Rule::Renju);
        b.excluded.push(Point{x:1,y:0});b.stones.push(Stone{x:0,y:0,color:Color::W});
        let a=Analysis{root_info:RootInfo{visits:10,winrate:0.5,current_player:Some("B".into())},
            move_infos:vec![],policy:vec![0.1;226],is_during_search:false,forbidden:Some(vec!["C15".into()])};
        let p=shortlist(&b,&a,SpecialMode::Defend,None,12);
        assert_eq!(p.len(),12);assert!(!p.contains(&Point{x:0,y:0}));assert!(!p.contains(&Point{x:1,y:0}));assert!(!p.contains(&Point{x:2,y:0}));
    }
}
