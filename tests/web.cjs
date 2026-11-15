const {JSDOM, VirtualConsole} = require('jsdom');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const path = require('node:path');
const os = require('node:os');
const net = require('node:net');
const {spawn} = require('node:child_process');
const root=path.resolve(__dirname,'..');
let base, proc, fixture;
const freePort=()=>new Promise((resolve,reject)=>{const s=net.createServer();s.on('error',reject);s.listen(0,'127.0.0.1',()=>{const port=s.address().port;s.close(()=>resolve(port));});});
const wait=async fn=>{for(let i=0;i<100;i++){if(fn())return;await new Promise(r=>setTimeout(r,50));}throw Error('UI did not settle');};
(async()=>{
 fixture=fs.mkdtempSync(path.join(os.tmpdir(),'tako-web-test-'));
 const port=await freePort();base=`http://127.0.0.1:${port}`;
 fs.writeFileSync(path.join(fixture,'config.toml'),`listen = "127.0.0.1:${port}"\n`);
 proc=spawn(path.join(root,'target/debug/tako'),['run'],{env:{...process.env,TAKO_HOME:fixture},stdio:'ignore'});
 for(let i=0;i<100;i++){try{const r=await fetch(base+'/health');if(r.ok)break;}catch{}await new Promise(r=>setTimeout(r,50));}
 const errors=[]; const logs=new VirtualConsole();logs.on('jsdomError',e=>errors.push(e.message));
 let copied='';
 const dom=new JSDOM(fs.readFileSync(root+'/src/web/index.html','utf8'),{
  url:base+'/ui',runScripts:'dangerously',pretendToBeVisual:true,virtualConsole:logs,
  beforeParse(w){
   w.fetch=(url,options={})=>fetch(new URL(url,base),{...options,headers:options.headers});
   w.matchMedia=()=>({matches:false,addEventListener(){}});
   w.ResizeObserver=class{observe(){}disconnect(){}};
   w.HTMLElement.prototype.scrollIntoView=function(){};
   w.HTMLDialogElement.prototype.showModal=function(){this.open=true;};
   w.HTMLDialogElement.prototype.close=function(){this.open=false;};
   Object.defineProperty(w.navigator,'clipboard',{value:{writeText:async s=>{copied=s;}}});
  }
 });
 const w=dom.window,d=w.document,$=s=>d.querySelector(s),click=s=>{assert.ok($(s),s);$(s).click();};
 await wait(()=>$('#onboarding').textContent.includes('Connect a provider'));
 assert.equal(d.querySelectorAll('.setup-step').length,3);
 assert.equal(d.title,'tako dashboard');
 assert.equal($('.brand').textContent,'tako');
 assert.ok($('.brand .tako-mark'));
 click('[data-page="access"]');
 await wait(()=>$('#client-keys').textContent.includes('A key for every connection'));
 click('#create-key'); assert.equal($('#modal').open,true);
 $('#key-name').value='UI test laptop'; $('#dialog-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>!$('#key-result').hidden && !$('#modal').open);
 const key=$('#new-key').value; assert.match(key,/^tako_[a-f0-9]{64}$/);
 assert.match($('#client-keys').textContent,/UI test laptop/);
 click('#copy-key');await wait(()=>copied===key);
 click('[data-rename]');$('#rename-key').value='Renamed laptop';$('#dialog-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>$('#client-keys').textContent.includes('Renamed laptop')&&!$('#modal').open);
 click('[data-revoke]');assert.match($('#dialog-description').textContent,/last managed key/);
 click('#dialog-cancel');assert.equal($('#modal').open,false);
 assert.match($('#client-keys').textContent,/Renamed laptop/);
 click('[data-revoke]');$('#dialog-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>$('#client-keys').textContent.includes('A key for every connection')&&!$('#modal').open);
 assert.equal((await fetch(base+'/v1/models',{headers:{Authorization:'Bearer '+key}})).status,401);
 click('#dismiss-key');assert.equal($('#new-key').value,'');
 click('[data-page="connect"]');assert.match($('#connect-content').textContent,/ANTHROPIC_BASE_URL/);
 assert.ok($('#connect-content').textContent.includes(base));
 assert.equal(errors.length,0,errors.join('\n'));
 dom.window.close();
 const login=new JSDOM(fs.readFileSync(root+'/src/web/login.html','utf8'),{url:base+'/ui?session=expired#access',runScripts:'dangerously'});
 const ld=login.window.document;assert.equal(ld.title,'Sign in · tako');assert.ok(ld.querySelector('.brand .tako-mark'));assert.equal(ld.querySelector('#expired').hidden,false);
 ld.querySelector('#show').click();assert.equal(ld.querySelector('#password').type,'text');
 ld.querySelector('#show').click();assert.equal(ld.querySelector('#password').type,'password');
 login.window.close();
 console.log('UI interaction checks passed: first run, access navigation, create/copy/rename/revoke/cancel, secret dismissal, remote recipes, login visibility controls.');
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(async()=>{
 if(proc && proc.exitCode === null){const ended=new Promise(resolve=>proc.once('exit',resolve));proc.kill('SIGTERM');await ended;}
 if(fixture)fs.rmSync(fixture,{recursive:true,force:true});
 // Close timers left by a failing DOM assertion too.
 if(process.exitCode)process.exit(process.exitCode);
});
