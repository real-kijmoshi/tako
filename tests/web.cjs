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
 let copied='', downloaded=null, downloadName='', tokenLimits=null, tokenTraffic=null, modelFixture=null;
 const dom=new JSDOM(fs.readFileSync(root+'/src/web/index.html','utf8'),{
  url:base+'/ui',runScripts:'dangerously',pretendToBeVisual:true,virtualConsole:logs,
  beforeParse(w){
   w.fetch=async(url,options={})=>{
    if(modelFixture && new URL(url,base).pathname==='/api/action' && options.body){
     const request=JSON.parse(options.body);
     if(request.action==='models' && request.provider===modelFixture.provider)return new Response(JSON.stringify(modelFixture),{status:200,headers:{'content-type':'application/json'}});
    }
    const response=await fetch(new URL(url,base),{...options,headers:options.headers});
    if(new URL(url,base).pathname==='/status' && tokenLimits!==null){
     const status=await response.json();status.token_limits=tokenLimits;
     if(tokenTraffic)Object.assign(status,tokenTraffic);
     return {ok:response.ok,status:response.status,json:async()=>status};
    }
    return response;
   };
   w.URL.createObjectURL=blob=>{downloaded=blob;return 'blob:tako-test';};
   w.URL.revokeObjectURL=()=>{};
   w.HTMLAnchorElement.prototype.click=function(){downloadName=this.download;};
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
 assert.equal($('#k-tokens-left').textContent,'–');
 await wait(()=>$('#token-limits').textContent.includes('Send a request'));
 assert.match($('#token-limits').textContent,/Send a request/);
 tokenLimits={};tokenTraffic={requests:{total:1,first_try:1,rescued:0,failed:0},token_targets:{'zai/glm-4.5-flash':{total_tokens:150}}};
 await wait(()=>$('#k-tokens-left').textContent==='Not reported');
 assert.match($('#k-tokens-left-sub').textContent,/Provider did not report/);
 assert.match($('#token-limits').textContent,/zai: no token limit reported/);
 assert.match($('#token-limits').textContent,/tokens used alone cannot determine what remains/);
 const observed=Math.floor(Date.now()/1000);
 tokenLimits={'old/model':{remaining:900,limit:1000,observed_at:observed-60},'new/model':{remaining:250,limit:1000,observed_at:observed}};
 await wait(()=>$('#k-tokens-left').textContent==='~250');
 assert.equal($('#k-tokens-left').classList.contains('unreported'),false);
 assert.match($('#k-tokens-left-sub').textContent,/new\/model/);
 tokenLimits={'new/model':{remaining:0,limit:1000,observed_at:observed}};
 await wait(()=>$('#k-tokens-left').textContent==='~0');
 tokenLimits={};await wait(()=>$('#k-tokens-left').textContent==='Not reported');
 tokenTraffic=null;
 await wait(()=>$('#k-tokens-left').textContent==='–');
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
 assert.equal($('#connect-model').disabled,true);
 assert.equal($('[data-recipe]').disabled,true);
 click('[data-tool="1"]');
 assert.equal($('[data-tool="1"]').getAttribute('aria-pressed'),'true');
 assert.match($('#connect-content').textContent,/opencode.json/);
 assert.equal(d.querySelectorAll('#connect-content article').length,1);
 click('[data-tool="0"]');

 const search=query=>{click('#cmdk-open');$('#cmdk-q').value=query;$('#cmdk-q').dispatchEvent(new w.Event('input',{bubbles:true}));};
 search('move');assert.match($('#cmdk-list').textContent,/Move your setup/);
 click('#cmdk-list .cmdk-item.on');assert.equal($('#cmdk').hidden,true);
 assert.equal($('#page-connect').hidden,false);assert.equal(d.activeElement,$('#settings-export'));
 for(const query of ['transfer','migrate','backup','restore']){
  search(query);assert.ok(d.querySelectorAll('#cmdk-list .cmdk-item').length>0,query);
  w.document.dispatchEvent(new w.KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
 }
 search('export settings');$('#cmdk-q').dispatchEvent(new w.KeyboardEvent('keydown',{key:'Enter',bubbles:true}));
 assert.match($('#dialog-description').textContent,/provider API keys/);
 click('#dialog-cancel');assert.equal(downloaded,null);
 let filePickerOpened=false;$('#settings-file').addEventListener('click',()=>{filePickerOpened=true;});
 search('import settings');click('#cmdk-list .cmdk-item.on');
 assert.equal(filePickerOpened,true);assert.equal($('#cmdk').hidden,true);
 click('#settings-export');$('#dialog-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>downloaded!==null&&!$('#modal').open);
 assert.equal(downloadName,'tako-settings.json');
 const bundle={format:'tako-settings',version:1,settings:'default = "portable"\n[providers.portable]\ntype = "openai"\nkeys = ["test-transfer-key"]\n[combos]\nportable = ["portable/model"]\n'};
 const input=$('#settings-file');
 const choose=async value=>{
  const text=JSON.stringify(value);
  Object.defineProperty(input,'files',{configurable:true,value:[{size:text.length,text:async()=>text}]});
  input.dispatchEvent(new w.Event('change',{bubbles:true}));
  await wait(()=>$('#modal').open||$('#settings-result').textContent.includes('Invalid settings'));
 };
 await choose(bundle);assert.match($('#dialog-body').textContent,/1 providers/);
 click('#dialog-cancel');
 assert.equal(JSON.parse((await (await fetch(base+'/api/overview')).text())).providers.portable,undefined);
 await choose(bundle);$('#dialog-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>!$('#modal').open&&$('#settings-result').textContent.includes('Imported 1 providers'));
 assert.ok(fs.readdirSync(fixture).some(f=>f.startsWith('config.before-import-')));
 assert.equal($('#connect-model').disabled,false);
 assert.equal($('#connect-model').value,'portable');
 click('[data-tool="1"]');click('[data-recipe]');
 await wait(()=>copied.includes('@ai-sdk/openai-compatible'));
 const provider=JSON.parse(copied).provider.tako;
 assert.equal(provider.options.baseURL,base+'/v1');
 assert.ok(provider.models.portable);
 $('#connect-model').value='portable/model';$('#connect-model').dispatchEvent(new w.Event('change',{bubbles:true}));
 assert.equal($('[data-tool="1"]').getAttribute('aria-pressed'),'true');
 click('[data-recipe]');await wait(()=>copied.includes('portable/model'));
 assert.ok(JSON.parse(copied).provider.tako.models['portable/model']);

 const config=fs.readFileSync(path.join(fixture,'config.toml'),'utf8');
 assert.match(config,/test-transfer-key/);assert.ok(config.includes('127.0.0.1:'+port));
 const bad={...bundle,settings:'[combos]\ninvalid=["missing/model"]'};
 await choose(bad);assert.equal($('#modal').open,false);
 assert.equal(fs.readFileSync(path.join(fixture,'config.toml'),'utf8'),config);
 assert.equal(errors.length,0,errors.join('\n'));
 // Every new provider has a bundled mark and a usable, copyable setup guide.
 click('[data-page="providers"]');
 for(const id of ['nous','modelscope','siliconflow','blockrun','novita','vikasit']){
  const guide=$(`[data-provider-guide="${id}"]`);assert.ok(guide,id);
  assert.ok(guide.closest('.provider-card').querySelector('.plogo svg image'),id+' icon');
  guide.click();assert.equal(d.querySelectorAll('#dialog-body .provider-steps li').length,3);
  assert.ok($('#dialog-body a[target="_blank"]'));
  click('[data-guide-copy]');await wait(()=>copied.startsWith('http'));
  click('#dialog-cancel');
 }
 click('[data-provider-add="nous"]');
 assert.match($('#add-setup-body').textContent,/adding credits or starting a plan/);
 assert.equal($('#add-secret').required,true);assert.equal($('#add-setup').open,true);
 $('#add-secret').value='test-catalog-key';
 $('#dialog-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>!$('#modal').open&&!!$('[data-provider-models="nous"]'));
 modelFixture={provider:'nous',models:[
  {id:'upstage/solar-mini4:free',target:'nous/upstage/solar-mini4:free',free:false,recommended:true}
 ]};
 click('[data-provider-models="nous"]');await wait(()=>$('#model-list').textContent.includes('Recommended'));
 assert.equal($('#models-free').checked,false);
 assert.ok(!$('#model-list').textContent.includes('free tier'));
 click('#dialog-cancel');
 click('[data-provider-guide="blockrun"]');click('#guide-connect');
 assert.equal($('#add-secret').required,false);assert.match($('#add-secret').placeholder,/leave blank/);
 $('#dialog-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>!$('#modal').open&&!!$('[data-provider-models="blockrun"]'));
 modelFixture={provider:'blockrun',warning:'Showing unverified catalog suggestions.',models:[
  {id:'poolside/laguna-xs-2.1',target:'blockrun/poolside/laguna-xs-2.1',free:true,recommended:true},
  {id:'paid/example',target:'blockrun/paid/example',free:false,recommended:false}
 ]};
 click('[data-provider-models="blockrun"]');await wait(()=>$('#model-list').textContent.includes('Recommended'));
 assert.equal($('#models-free').checked,true);assert.equal(d.querySelectorAll('.model-option').length,1);
 assert.match($('#model-load-error').textContent,/unverified/);
 $('#models-free').checked=false;$('#models-free').dispatchEvent(new w.Event('change',{bubbles:true}));
 assert.equal(d.querySelectorAll('.model-option').length,2);click('#dialog-cancel');
 assert.equal(errors.length,0,errors.join('\n'));
 // The Code page pairs an outbound PC, queues real jobs and renders untrusted patch text safely.
 click('[data-page="code"]');await wait(()=>$('#code-start').disabled);
 assert.match($('#code-setup-commands').textContent,/^curl -fsSL https:\/\/github\.com\/real-kijmoshi\/tako\/releases\/latest\/download\/install-code\.sh \| sh\ncd .*\ntako-code\n# First run asks for this server: http:/);
 assert.doesNotMatch($('#code-setup-commands').textContent,/python/);
 assert.match($('#code-download').href,/releases\/latest\/download\/tako-code-(macos|linux)-/);
 $('#code-os').value='windows-x86_64';$('#code-os').dispatchEvent(new w.Event('change'));
 assert.match($('#code-download').href,/tako-code-windows-x86_64\.exe$/);
 assert.match($('#code-setup-commands').textContent,/^irm https:.*install-code\.ps1 \| iex\ncd C:.*\ntako-code\n/);
 const codeKey=(await (await fetch(base+'/api/action',{method:'POST',headers:{'content-type':'application/json','x-tako':'1'},body:JSON.stringify({action:'key.create',name:'Code runner'})})).json()).key;
 const codeRequest=async(endpoint,body)=>{const r=await fetch(base+endpoint,{method:'POST',headers:{'content-type':'application/json','x-tako':'1',Authorization:'Bearer '+codeKey},body:JSON.stringify(body)});assert.equal(r.status,200,await r.clone().text());return r.json();};
 const pc={runner:'ui-pc',name:'My <PC>',projects:[{id:'app',name:'Local app'}],engines:['builtin']};
 pc.runner_token=(await codeRequest('/v1/code/register',pc)).runner_token;
 await codeRequest('/v1/code/register',pc);click('#code-refresh');await wait(()=>!$('#code-start').disabled);
 assert.equal($('#code-runner').selectedOptions[0].textContent,'My <PC>');
 assert.equal(d.querySelectorAll('#code-engine option').length,1);
 $('#code-task').value='Implement <script>unsafe()</script>';
 $('#code-form').dispatchEvent(new w.Event('submit',{bubbles:true,cancelable:true}));
 await wait(()=>!!$('#code-cancel'));
 assert.equal($('#code-detail script'),null);assert.match($('#code-detail').textContent,/Implement <script>/);
 const lease=await codeRequest('/v1/code/poll',pc);assert.ok(lease.job.id);
 await codeRequest('/v1/code/event',{...pc,job:lease.job.id,event:{phase:'ready',workers:[{id:'<img src=x>',title:'Worker <one>',phase:'done'}],diff:'+<script>unsafe()</script>',diff_truncated:true,review:'Review <text>',tests:[{command:['test'],exit_code:0,output:'actual output'}],apply_command:'python tako-code.py apply '+lease.job.id}});
 click('#code-refresh');await wait(()=>!!$('#code-copy-apply'));
 assert.equal($('#code-detail script'),null);assert.equal($('#code-detail img'),null);
 assert.match($('#code-detail').textContent,/actual output/);assert.match($('#code-detail').textContent,/preview truncated/);
 assert.equal($('#code-cancel'),null);click('#code-copy-apply');await wait(()=>copied.includes(' apply '+lease.job.id));
 assert.equal(errors.length,0,errors.join('\n'));
 await new Promise(resolve=>setTimeout(resolve,20));
 dom.window.close();
 const login=new JSDOM(fs.readFileSync(root+'/src/web/login.html','utf8'),{url:base+'/ui?session=expired#access',runScripts:'dangerously'});
 const ld=login.window.document;assert.equal(ld.title,'Sign in · tako');assert.ok(ld.querySelector('.brand .tako-mark'));assert.equal(ld.querySelector('#expired').hidden,false);
 ld.querySelector('#show').click();assert.equal(ld.querySelector('#password').type,'text');
 ld.querySelector('#show').click();assert.equal(ld.querySelector('#password').type,'password');
 login.window.close();
 // A router restarted on a new build makes open dashboards reload, but never over unsaved text.
 let uiBuild='build-a', reloaded=false; const reloadLogs=new VirtualConsole();
 reloadLogs.on('jsdomError',e=>{if(/navigation/i.test(e.message))reloaded=true;});
 const stale=new JSDOM(fs.readFileSync(root+'/src/web/index.html','utf8'),{url:base+'/ui',runScripts:'dangerously',pretendToBeVisual:true,virtualConsole:reloadLogs,
  beforeParse(w){
   w.fetch=async(url,options={})=>{const r=await fetch(new URL(url,base),{...options,headers:options.headers});if(String(url)!=='/status')return r;
    const s=await r.json();s.ui=uiBuild;return new Response(JSON.stringify(s),{status:200,headers:{'content-type':'application/json'}});};
   w.matchMedia=()=>({matches:false,addEventListener(){}});w.ResizeObserver=class{observe(){}disconnect(){}};
   w.HTMLDialogElement.prototype.showModal=function(){this.open=true;};w.HTMLDialogElement.prototype.close=function(){this.open=false;};
  }});
 const s$=q=>stale.window.document.querySelector(q);
 await wait(()=>/^v/.test(s$('#ver').textContent));
 s$('#request-search').focus();s$('#request-search').value='unsaved';uiBuild='build-b';
 await new Promise(r=>setTimeout(r,2000));assert.equal(reloaded,false,'reload waits while a field holds text');
 s$('#request-search').value='';await wait(()=>reloaded);
 stale.window.close();
 console.log('UI interaction checks passed: first run, access navigation, create/copy/rename/revoke/cancel, secret dismissal, remote recipes, login visibility controls, reload on new build.');
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(async()=>{
 if(proc && proc.exitCode === null){const ended=new Promise(resolve=>proc.once('exit',resolve));proc.kill('SIGTERM');await ended;}
 if(fixture)fs.rmSync(fixture,{recursive:true,force:true});
 // Close timers left by a failing DOM assertion too.
 if(process.exitCode)process.exit(process.exitCode);
});
