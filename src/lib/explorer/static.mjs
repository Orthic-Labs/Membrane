import { extname } from "node:path";

const HTML = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Cortex Explorer</title><link rel="stylesheet" href="/explorer.css"></head>
<body><header><div><b>Cortex</b><span id="freshness">connecting</span></div><nav>
<button data-view="architecture">Architecture</button><button data-view="search">Search</button><button id="orb">Orb: off</button></nav></header>
<main><aside><form id="search"><input id="query" aria-label="Search evidence" placeholder="symbol, route, claim…"><button>Search</button></form><div id="results"></div></aside>
<section><canvas id="canvas" aria-label="Evidence graph"></canvas><div id="empty">Loading architecture…</div></section>
<aside id="evidence"><h2>Evidence</h2><pre>Select an item</pre></aside></main><script type="module" src="/explorer.js"></script></body></html>`;

const CSS = `:root{color-scheme:dark;font:14px/1.45 system-ui;background:#090b10;color:#e8edf7}*{box-sizing:border-box}body{margin:0}header{height:56px;display:flex;align-items:center;justify-content:space-between;padding:0 18px;border-bottom:1px solid #252a35;background:#0e1118}header b{font-size:18px}header span{margin-left:12px;color:#8ea0bc}button,input{font:inherit;color:inherit;background:#151a24;border:1px solid #303747;border-radius:7px;padding:8px 10px}button{cursor:pointer}button:hover{border-color:#6f83a8}main{display:grid;grid-template-columns:280px minmax(360px,1fr) 300px;height:calc(100vh - 56px)}aside{padding:14px;border-right:1px solid #252a35;overflow:auto}#evidence{border-right:0;border-left:1px solid #252a35}form{display:flex;gap:8px}input{min-width:0;flex:1}section{position:relative;overflow:hidden;background:radial-gradient(circle at 50% 45%,#141a28,#090b10 65%)}canvas{width:100%;height:100%;display:block}#empty{position:absolute;inset:50% auto auto 50%;transform:translate(-50%,-50%);color:#7d899d}.result{display:block;width:100%;text-align:left;margin:8px 0}.result small{display:block;color:#8ea0bc}pre{white-space:pre-wrap;word-break:break-word;color:#b9c5d8}@media(max-width:900px){main{grid-template-columns:220px 1fr}#evidence{display:none}}`;

const JS = `const token=new URLSearchParams(location.hash.slice(1)).get('token');const headers={authorization:'Bearer '+token};const canvas=document.querySelector('#canvas'),ctx=canvas.getContext('2d'),results=document.querySelector('#results'),evidence=document.querySelector('#evidence pre'),empty=document.querySelector('#empty');let nodes=[],orb=false,yaw=0,pitch=0;
async function api(path){const r=await fetch(path,{headers});if(!r.ok)throw new Error((await r.json()).error?.code||r.status);return r.json()}
function resize(){const d=devicePixelRatio||1,r=canvas.getBoundingClientRect();canvas.width=r.width*d;canvas.height=r.height*d;ctx.setTransform(d,0,0,d,0,0);draw()}addEventListener('resize',resize);
function layout(items){const a=Math.PI*(3-Math.sqrt(5)),n=items.length;return [...items].sort((x,y)=>String(x.id).localeCompare(String(y.id))).map((x,i)=>{const yy=n===1?0:1-i/Math.max(1,n-1)*2,q=Math.sqrt(Math.max(0,1-yy*yy)),t=a*i;return {...x,x:Math.cos(t)*q*230,y:yy*230,z:Math.sin(t)*q*230}})}
function draw(){const r=canvas.getBoundingClientRect();ctx.clearRect(0,0,r.width,r.height);const pts=nodes.map(n=>{const cy=Math.cos(yaw),sy=Math.sin(yaw),cp=Math.cos(pitch),sp=Math.sin(pitch),x=n.x*cy-n.z*sy,z=n.x*sy+n.z*cy,y=n.y*cp-z*sp,z2=n.y*sp+z*cp,s=700/(700+z2);return {...n,px:r.width/2+x*s,py:r.height/2+y*s,s,z:z2}}).sort((a,b)=>a.z-b.z);for(const n of pts){ctx.beginPath();ctx.fillStyle=n.kind==='file'?'#61d6a8':'#789cff';ctx.globalAlpha=Math.max(.35,Math.min(1,n.s));ctx.arc(n.px,n.py,Math.max(3,6*n.s),0,Math.PI*2);ctx.fill()}ctx.globalAlpha=1;canvas._points=pts}
canvas.addEventListener('click',e=>{const r=canvas.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top,p=(canvas._points||[]).findLast(n=>Math.hypot(n.px-x,n.py-y)<10);if(p)evidence.textContent=JSON.stringify(p,null,2)});let drag;canvas.addEventListener('pointerdown',e=>drag=[e.clientX,e.clientY]);canvas.addEventListener('pointermove',e=>{if(!drag||!orb)return;yaw+=(e.clientX-drag[0])/180;pitch+=(e.clientY-drag[1])/180;drag=[e.clientX,e.clientY];draw()});addEventListener('pointerup',()=>drag=null);
function list(items){results.innerHTML='';for(const item of items){const b=document.createElement('button');b.className='result';b.textContent=item.qualifiedName||item.name||item.path||item.id;b.onclick=()=>evidence.textContent=JSON.stringify(item,null,2);results.append(b)}}
document.querySelector('#orb').onclick=e=>{orb=!orb;e.target.textContent='Orb: '+(orb?'on':'off');yaw=pitch=0;draw()};document.querySelector('#search').onsubmit=async e=>{e.preventDefault();try{const p=await api('/api/search?q='+encodeURIComponent(document.querySelector('#query').value));list(p.results||[])}catch(x){empty.textContent=x.message}};
Promise.all([api('/api/status'),api('/api/architecture')]).then(([s,a])=>{document.querySelector('#freshness').textContent=s.state||'ready';const raw=a.nodes||a.layers||a.architecture?.nodes||[];nodes=layout(raw.map((n,i)=>({id:String(n.id||n.name||i),label:n.label||n.name||n.path||n.id,kind:n.kind||n.type||'node',...n})));empty.hidden=nodes.length>0;empty.textContent=nodes.length?'':'No architecture nodes';resize()}).catch(e=>{empty.textContent='Explorer error: '+e.message});`;

const ASSETS = new Map([["/", ["text/html; charset=utf-8", HTML]], ["/index.html", ["text/html; charset=utf-8", HTML]], ["/explorer.css", ["text/css; charset=utf-8", CSS]], ["/explorer.js", ["text/javascript; charset=utf-8", JS]]]);

export function serveExplorerAsset(request, response) {
  const path = new URL(request.url ?? "/", "http://127.0.0.1").pathname;
  const asset = ASSETS.get(path);
  if (!asset) { response.writeHead(404, { "content-type": "text/plain; charset=utf-8" }); response.end("not found"); return; }
  const [type, body] = asset;
  response.writeHead(200, { "content-type": type, "cache-control": "no-store", "content-security-policy": "default-src 'self'; connect-src 'self'; script-src 'self'; style-src 'self'; img-src 'none'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'", "x-content-type-options": "nosniff" });
  response.end(body);
}
