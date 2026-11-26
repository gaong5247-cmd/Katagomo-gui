const $=id=>document.getElementById(id);
const api=window.__TAURI__;
const invoke=(name,args)=>api.core.invoke(name,args);
const letters="ABCDEFGHJKLMNOPQRST";
let size=15,rule="freestyle",stones=[],analysis=null,busy=false,history=[],highlight=null;
let profiles=[];
try{profiles=JSON.parse(localStorage.getItem("kata-profiles-v1")||"[]");if(!Array.isArray(profiles))profiles=[];}catch{}
const canvas=$("board"), ctx=canvas.getContext("2d");
const key=()=>rule+":"+size;
const next=()=>stones.length%2?"W":"B";
const coord=(x,y)=>letters[x]+(size-y);
function parseCoord(c){const x=letters.indexOf(c[0]),y=size-Number(c.slice(1));return x>=0&&x<size&&y>=0&&y<size?{x,y}:null}
function pos(){return{size,rule,stones:stones.map(s=>({...s})),next:next(),excluded:[],history_known:true}}
function status(s){$("status").textContent=s}
function setBusy(b){busy=b;for(const id of ["rule","size","new","undo","save","best","analyze","profile-save","profile-remove","auto"])$(id).disabled=b;$("stop").disabled=!b;}
function current(){return profiles.find(p=>p.size===size&&p.rule===rule)}
function readProfile(){let p=current();$("profile-key").textContent=key()+" · "+(p?"등록됨":"미등록");$("exe").value=p?.executable||"";$("model").value=p?.model||"";$("config").value=p?.config||"";$("extra").value=JSON.stringify(p?.extra_rules||{});}
function persist(){localStorage.setItem("kata-profiles-v1",JSON.stringify(profiles));readProfile()}
$("profile-save").onclick=()=>{try{let extra=JSON.parse($("extra").value);if(!extra||Array.isArray(extra)||typeof extra!=="object")throw Error("추가 룰은 JSON 객체여야 합니다.");let p={rule,size,executable:$("exe").value.trim(),model:$("model").value.trim(),config:$("config").value.trim(),extra_rules:extra};if(!p.executable||!p.model||!p.config)throw Error("세 경로를 모두 지정하세요.");profiles=profiles.filter(p=>p.rule!==rule||p.size!==size);profiles.push(p);persist();status("프로필 저장됨");}catch(e){status(String(e))}};
$("profile-remove").onclick=()=>{profiles=profiles.filter(p=>p.rule!==rule||p.size!==size);persist()};
for(const b of document.querySelectorAll("[data-pick]"))b.onclick=async()=>{try{const result=await api.dialog.open({multiple:false,directory:false});if(typeof result==="string")$(b.dataset.pick).value=result}catch(e){status(String(e))}};
function clearAnalysis(){analysis=null;highlight=null;renderAnalysis()}
function reset(){stones=[];history=[];clearAnalysis();draw();renderMoves()}
for(const id of ["rule","size"])$(id).onchange=()=>{rule=$("rule").value;size=Number($("size").value);reset();readProfile()};
$("new").onclick=reset;
$("undo").onclick=()=>{if(busy||!stones.length)return;stones.pop();if($("auto").checked&&stones.length%2===1)stones.pop();history=history.filter(p=>p.ply<stones.length);clearAnalysis();draw();renderMoves()};
$("heat").onchange=draw;
$("save").onclick=async()=>{try{const path=await api.dialog.save({defaultPath:"gomoku.sgf",filters:[{name:"SGF",extensions:["sgf"]}]});if(path){await invoke("save_sgf",{path,position:pos()});status("SGF 저장됨")}}catch(e){status(String(e))}};
function geom(){const w=canvas.width,m=w/(size+1);return{w,m,step:(w-2*m)/(size-1)}}
function point(x,y){const {m,step}=geom();return[m+x*step,m+y*step]}
function draw(){let bounds=canvas.getBoundingClientRect(),dpr=window.devicePixelRatio||1;let dim=Math.max(1,Math.round(bounds.width*dpr));if(canvas.width!==dim||canvas.height!==dim){canvas.width=dim;canvas.height=dim}const {w,m,step}=geom();ctx.fillStyle="#d7a566";ctx.fillRect(0,0,w,w);ctx.strokeStyle="#573e28";ctx.lineWidth=Math.max(1,w/850);
for(let i=0;i<size;i++){let p=m+i*step;ctx.beginPath();ctx.moveTo(m,p);ctx.lineTo(w-m,p);ctx.moveTo(p,m);ctx.lineTo(p,w-m);ctx.stroke()}
ctx.fillStyle="#604329";ctx.font=String(Math.max(10,m*.42))+"px system-ui";ctx.textAlign="center";ctx.textBaseline="middle";
for(let i=0;i<size;i++){ctx.fillText(letters[i],m+i*step,m*.43);ctx.fillText(String(size-i),m*.43,m+i*step)}
for(let x of [3,Math.floor(size/2),size-4])for(let y of [3,Math.floor(size/2),size-4]){const [px,py]=point(x,y);ctx.beginPath();ctx.arc(px,py,step*.075,0,Math.PI*2);ctx.fill()}
if(analysis&&$("heat").checked){for(const [i,c] of analysis.moveInfos.slice(0,12).entries()){const p=parseCoord(c.move);if(!p||stones.some(s=>s.x===p.x&&s.y===p.y))continue;const [x,y]=point(p.x,p.y);ctx.beginPath();ctx.fillStyle=i===0?"#1e9a78b5":"#338aaa88";ctx.arc(x,y,step*.39,0,Math.PI*2);ctx.fill();ctx.fillStyle="#fff";ctx.font="bold "+step*.31+"px system-ui";ctx.fillText(String(i+1),x,y)}}
if(analysis?.forbidden&&$("heat").checked){ctx.strokeStyle="#bd272b";ctx.lineWidth=step*.065;for(const v of analysis.forbidden){let p=parseCoord(v);if(!p)continue;let [x,y]=point(p.x,p.y),r=step*.2;ctx.beginPath();ctx.moveTo(x-r,y-r);ctx.lineTo(x+r,y+r);ctx.moveTo(x-r,y+r);ctx.lineTo(x+r,y-r);ctx.stroke()}}
for(const [i,s] of stones.entries()){const [x,y]=point(s.x,s.y);const r=step*.44;const g=ctx.createRadialGradient(x-r*.3,y-r*.38,r*.03,x,y,r);if(s.color==="B"){g.addColorStop(0,"#646b73");g.addColorStop(1,"#0c0e12")}else{g.addColorStop(0,"#fff");g.addColorStop(1,"#c9cbd0")}ctx.beginPath();ctx.fillStyle=g;ctx.arc(x,y,r,0,Math.PI*2);ctx.fill();ctx.strokeStyle=s.color==="B"?"#050608":"#8e8f93";ctx.stroke();if(i===stones.length-1){ctx.fillStyle="#eb7750";ctx.beginPath();ctx.arc(x,y,step*.105,0,Math.PI*2);ctx.fill()}}
if(highlight){const [x,y]=point(highlight.x,highlight.y);ctx.strokeStyle="#fff";ctx.lineWidth=step*.07;ctx.beginPath();ctx.arc(x,y,step*.47,0,Math.PI*2);ctx.stroke()}}
new ResizeObserver(draw).observe(canvas);
function renderMoves(){$("turn").textContent=(next()==="B"?"흑":"백")+" 차례 · "+stones.length+"수";$("moves").replaceChildren(...stones.map((s,i)=>{let e=document.createElement("span");e.textContent=(i+1)+". "+s.color+" "+coord(s.x,s.y);return e}))}
function play(p){if(!p||busy||analysis?.forbidden?.includes(coord(p.x,p.y))||stones.some(s=>s.x===p.x&&s.y===p.y))return false;stones.push({...p,color:next()});clearAnalysis();draw();renderMoves();return true}
canvas.addEventListener("pointerdown",e=>{if(busy)return;const r=canvas.getBoundingClientRect(),{m,step}=geom();const x=Math.round(((e.clientX-r.left)*canvas.width/r.width-m)/step),y=Math.round(((e.clientY-r.top)*canvas.height/r.height-m)/step);if(x<0||x>=size||y<0||y>=size)return;if(play({x,y})&&$("auto").checked&&next()==="W")run(true)});
function plot(){let c=$("chart"),g=c.getContext("2d"),w=c.width,h=c.height;g.clearRect(0,0,w,h);g.strokeStyle="#405263";g.setLineDash([5,5]);g.beginPath();g.moveTo(0,h/2);g.lineTo(w,h/2);g.stroke();g.setLineDash([]);if(!history.length)return;g.strokeStyle="#54d6b4";g.lineWidth=2;g.beginPath();history.forEach((p,i)=>{let x=10+(w-20)*p.ply/Math.max(1,stones.length,history.at(-1).ply),y=5+(h-10)*(1-p.wr);if(i===0)g.moveTo(x,y);else g.lineTo(x,y)});g.stroke()}
function renderAnalysis(){const a=analysis;const c=$("candidates");c.replaceChildren();$("wr").textContent=a?(a.rootInfo.winrate*100).toFixed(1)+"%":"—";$("blackbar").style.width=a?String(Math.max(0,Math.min(100,a.rootInfo.winrate*100)))+"%":"50%";$("root").textContent=a?"방문 "+a.rootInfo.visits.toLocaleString()+" · 흑 기준 평가 (무승부 0.5)"+(a.isDuringSearch?" · 탐색 중":""):"분석 결과 없음";
if(a)for(const v of [...a.moveInfos].sort((a,b)=>a.order-b.order).slice(0,16)){let row=document.createElement("div");row.className="candidate";let move=document.createElement("b"),wr=document.createElement("span"),n=document.createElement("span"),pv=document.createElement("div");move.textContent=v.move;wr.textContent=(v.winrate*100).toFixed(1)+"%";n.textContent=v.visits+" visits";pv.className="pv";pv.textContent="PV "+v.pv.join(" · ")+(v.drawrate==null?"":" | draw "+(v.drawrate*100).toFixed(1)+"%");row.append(move,wr,n,pv);row.onclick=()=>{highlight=parseCoord(v.move);draw()};c.append(row)}plot()}
function apply(a){analysis=a;renderAnalysis();draw()}
async function run(playAfter=false){if(busy)return;if(!current()){status("현재 룰/크기의 모델 프로필을 먼저 등록하세요.");return}setBusy(true);status("엔진 초기화 / 탐색 중");let p=pos();try{const a=await invoke("analyze",{profiles,position:p,limits:{visits:Number($("visits").value),seconds:Number($("seconds").value)}});apply(a);history=history.filter(h=>h.ply!==stones.length);history.push({ply:stones.length,wr:a.rootInfo.winrate});history.sort((a,b)=>a.ply-b.ply);plot();status(a.isDuringSearch?"탐색 중단 · 마지막 결과":"분석 완료");setBusy(false);if(playAfter&&!a.isDuringSearch)playBest()}catch(e){status(String(e));setBusy(false)}}
function playBest(){const a=analysis;if(!a||busy)return;for(const v of [...a.moveInfos].sort((a,b)=>a.order-b.order)){if(play(parseCoord(v.move)))return}status("착수할 후보가 없습니다.")}
$("analyze").onclick=()=>run(false);$("best").onclick=playBest;$("stop").onclick=()=>invoke("stop").catch(e=>status(String(e)));
if(api)await api.event.listen("analysis-progress",e=>{if(busy)apply(e.payload)});
else status("Tauri 데스크톱 앱에서 실행하세요.");
readProfile();renderMoves();renderAnalysis();draw();
