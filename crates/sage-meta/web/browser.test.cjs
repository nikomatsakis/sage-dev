const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const {resolve} = require('node:path');
const {buildModel,createRenderer,searchModel,sourceOwner,renderSourcePane,entityRoute,bookHref,createNavigation} = require('./browser.js');
const artifact = resolve(__dirname, '../../../target/sage-browser.html');
const html = readFileSync(artifact,'utf8');
const files = JSON.parse(html.split('<script id="model-data" type="application/json">')[1].split('</script>')[0]);
const sources=JSON.parse(html.split('<script id="source-data" type="application/json">')[1].split('</script>')[0]);
const model = buildModel(files,sources), renderer = createRenderer(model);

test('anchor context routes select their owning entity and rendered definition',()=>{
  const source=structuredClone(sources);
  const item=model.items.get('sage_ir::types::Ty');
  const name='crates.sage_ir.types.identity';
  source.anchors={[name]:{entity:item.key}};
  source.prose={[item.file]:{[item.description]:`<p><a class="spec-anchor" id="${name}" href="design/anchor/crates/sage_ir/types/identity.html">identity</a> A type.</p>`}};
  const anchored=buildModel(files,source);
  const page=createRenderer(anchored).render(`anchor:${name}`);
  assert.match(page,/id="crates.sage_ir.types.identity"/);
  assert.match(page,/<h2>Variants<\/h2>/);
  assert.equal(sourceOwner(anchored,`anchor:${name}`).key,item.key);
  assert.equal(bookHref(anchored,{home:'design/crates/index.html',chapters:{'sage_ir::types':'design/crates/sage_ir/types.html'}},`anchor:${name}`),`design/crates/sage_ir/types.html#${encodeURIComponent(`anchor:${name}`)}`);
  assert.match(createRenderer(anchored).render('anchor:missing'),/Not found/);
});

test('Rust evidence links show source snapshots at the requested line range',()=>{
  const snapshot={...sources, files:{...sources.files,'tests/claim.rs':'fn before() {}\n// <script>inert</script>\nfn claim() {}\n'}};
  const page=createRenderer(buildModel(files,snapshot)).render('source:tests/claim.rs:2:3');
  assert.match(page,/Rust source snapshot/);
  assert.match(page,/Lines 2–3/);
  assert.equal((page.match(/selected-source-line/g)||[]).length,2);
  assert.equal((page.match(/data-source-focus/g)||[]).length,1);
  assert.match(page,/&lt;script&gt;inert/);
  assert.doesNotMatch(page,/<script>/);
  assert.match(createRenderer(buildModel(files,snapshot)).render('source:missing.rs:1:2'),/Not found/);
});

test('indexes actual crates, types, methods, and Rust annotations',()=>{
  assert.ok(model.items.size > 0);
  assert.ok(model.crates.has('sage_ir'));
  assert.equal(model.items.get('sage_ir::types::Ty').kind,'enum');
  assert.ok(sources.targets['sage_ir::symbols::LocalFnSym'].some(site=>site.path.endsWith('::LocalFnSym')));
});

test('navigates function bodies to their signatures and possible callee interfaces',()=>{
  const key='sage_ir::symbols::LocalFnSym::body';
  const rendered=renderer.render(`method:${key}`);
  assert.match(rendered,/Dependency envelope/);
  assert.match(rendered,/forbid transitive/);
  assert.ok(model.calls.get(key).includes('sage_ir::symbols::LocalFnSym::signature'));
  const callers=model.incoming.get('sage_ir::symbols::ExternalFn::signature');
  assert.ok(callers.includes('sage_ir::checking::BodyCheck::check'));
  assert.ok(!model.calls.get(key).includes(key));
});

test('renders all model routes and every generated internal link resolves',()=>{
  const routes=['home',...[...model.crates.keys()].map(k=>`crate:${k}`),...[...model.modules.keys()].map(k=>`module:${k}`),...[...model.items.keys()].map(k=>`item:${k}`),...[...model.methods.keys()].map(k=>`method:${k}`),...model.members.keys()];
  const known=new Set(routes);
  for(const route of routes){
    const page=renderer.render(route);
    assert.ok(!page.includes('<h1>Not found</h1>'),route);
    for(const match of page.matchAll(/href="#([^"]+)"/g)) assert.ok(known.has(decodeURIComponent(match[1])),`${route} links to ${match[1]}`);
  }
  assert.match(renderer.render('item:missing'),/Not found/);
});

test('exposes variant prose, generic type links, and review-only Rust correspondences',()=>{
  assert.match(renderer.render('item:sage_ir::types::Ty'),/universal parameter/);
  assert.match(renderer.render('item:sage_ir::types::FnSig'),/item%3Asage_ir%3A%3Atypes%3A%3ATy/);
  assert.match(renderSourcePane(model.items.get('sage_ir::types::Signature'),sources),/Module correspondence anchor/);
});

test('escapes HTML in names, prose, and Rust paths',()=>{
  const hostile=structuredClone(files);
  const ty=hostile.find(f=>f.namespace==='sage_ir::types').document.declarations.find(d=>d.Type?.name==='Ty').Type;
  ty.description='<img src=x onerror=alert(1)>';
  const hostileSources=structuredClone(sources);
  hostileSources.targets['sage_ir::types::Ty'][0].path='<script>';
  const rendered=createRenderer(buildModel(hostile,hostileSources)).render('item:sage_ir::types::Ty');
  assert.ok(!rendered.includes('<img'));
  assert.ok(!rendered.includes('<script>'));
  assert.match(rendered,/&lt;img/);
});


test('searches names and Rust paths and filters by crate and operation kind',()=>{
  const rust=searchModel(model,'sage_ir::ty::FnSig');
  assert.ok(rust.some(item=>item.key==='sage_ir::types::FnSig'));
  const queries=searchModel(model,'signature','sage_ir','query');
  assert.ok(queries.length>0);
  assert.ok(queries.every(item=>item.crateName==='sage_ir'&&item.kind==='query'));
  assert.equal(searchModel(model,'no_such_declaration').length,0);
});


test('opens annotated Rust definitions and individual methods',()=>{
  const item=model.items.get('sage_ir::types::Ty');
  const refs=sources.targets[item.key];
  assert.ok(refs[0].locations.length);
  const location=refs[0].locations[0];
  assert.match(sources.files[location.file].split('\n').slice(location.line-1,location.end_line).join('\n'), /enum Ty/);
  assert.match(renderer.render('item:'+item.key),/data-source="sage_ir::types::Ty"/);
  assert.match(renderSourcePane(item,sources),/data-definition="true"/);
  assert.equal(sourceOwner(model,'method:sage_ir::symbols::LocalFnSym::body').name,'body');
  assert.equal(sourceOwner(model,'home'),null);
});

test('resolves workspace annotations, labels macro inputs, and explains abstractions',()=>{
  for(const [key,refs] of Object.entries(sources.targets)){
    if(['sage_ir::types::Boolean','sage_ir::types::Unit'].includes(key))continue;
    assert.ok(refs.some(ref=>ref.locations.length),key);
  }
  assert.match(renderSourcePane(model.items.get('sage_ir::symbols::Symbol'),sources),/Macro input/);
  assert.match(renderSourcePane(model.items.get('sage_ir::types::Signature'),sources),/Module correspondence anchor/);
  assert.match(renderSourcePane(model.items.get('sage_ir::types::Boolean'),sources),/Module correspondence anchor/);
});

test('escapes Rust source and supports choosing among definitions',()=>{
  const item=model.items.get('sage_ir::types::Ty');
  const fixture={files:{'a.rs':'<script>alert(1)</script>','b.rs':'enum Other {}'},targets:{[item.key]:[{path:'crate::Ty',locations:[
    {file:'a.rs',line:1,end_line:1,macro_input:false},
    {file:'b.rs',line:1,end_line:1,macro_input:false,cfg:['cfg(feature = "<other>")']}
  ]}]}};
  const first=renderSourcePane(item,fixture);
  assert.ok(!first.includes('<script>'));
  assert.match(first,/&lt;script&gt;/);
  assert.match(renderSourcePane(item,fixture,'0:1'),/enum Other/);
  assert.match(renderSourcePane(item,fixture,'0:1'),/Conditions \(not evaluated\).*&lt;other&gt;/);
});

test('named fields and variants have searchable links that select their containing declaration',()=>{
  const fields=searchModel(model,'','sage_ir','field');
  const variants=searchModel(model,'','sage_ir','variant');
  assert.ok(fields.length>0);
  assert.ok(variants.length>0);
  for(const member of [...fields,...variants]) {
    const route=entityRoute(member);
    assert.equal(model.members.get(route),member);
    assert.match(renderer.render(route),/data-selected-member="true"/);
    assert.equal(sourceOwner(model,route),model.items.get(member.owner));
    assert.ok(searchModel(model,member.key).includes(member));
  }
  assert.ok(searchModel(model,'','sage_ir','crate').some(item=>item.key==='sage_ir'));
  assert.ok(searchModel(model,'sage_ir::types','sage_ir','module').some(item=>item.key==='sage_ir::types'));
});

test('named variant fields are disambiguated by owner and variant',()=>{
  const fixture=structuredClone(files);
  const ty=fixture.find(f=>f.namespace==='sage_ir::types').document.declarations.find(d=>d.Type?.name==='Ty').Type;
  const type=fixture.find(f=>f.namespace==='sage_ir::types').document.declarations.find(d=>d.Type?.name==='FnSig').Type.shape.Struct[0].ty;
  ty.shape.Enum=[
    {name:'First',description:'First variant',fields:{Named:[{name:'value',ty:type,description:'First value'}]}},
    {name:'Second',description:'Second variant',fields:{Named:[{name:'value',ty:type,description:'Second value'}]}},
  ];
  const m=buildModel(fixture),r=createRenderer(m);
  const first='field:sage_ir::types::Ty::First::value';
  const second='field:sage_ir::types::Ty::Second::value';
  assert.notEqual(m.members.get(first),m.members.get(second));
  assert.match(r.render(first),/data-selected-member="true"[^]*First value/);
  assert.equal((r.render(second).match(/data-selected-member="true"/g)||[]).length,1);
  assert.match(r.render(second),/data-selected-member="true"[^]*Second value/);
  assert.equal(searchModel(m,'Second value')[0].key,'sage_ir::types::Ty::Second::value');
});

const book={home:'design/crates.html',chapters:Object.fromEntries([...model.modules.keys()].map(namespace=>[namespace,`design/crates/${namespace.replaceAll('::','/')}.html`]))};

test('book URLs use the supplied chapter inventory for every entity, including relocated chapters',()=>{
  for(const item of searchModel(model)) {
    const route=entityRoute(item);
    assert.equal(bookHref(model,book,route),`${book.chapters[item.namespace]}#${encodeURIComponent(route)}`);
  }
  assert.equal(bookHref(model,book,'home'),'design/crates.html#home');
  const relocated={...book,chapters:{...book.chapters,'sage_ir::types':'design/new-place/types.html'}};
  assert.equal(bookHref(model,relocated,'variant:sage_ir::types::Ty::Ref'),'design/new-place/types.html#variant%3Asage_ir%3A%3Atypes%3A%3ATy%3A%3ARef');
});

function frameFixture(hash='') {
  const fullPage={href:''};
  const parent=new EventTarget();
  parent.location=new URL(`https://example.test/book/design/crates/sage_ir/types.html${hash}`);
  parent.document={querySelector:()=>fullPage};
  const frame=new EventTarget();
  frame.location=new URL('https://example.test/book/sage-explorer.html?embed=1&route=module%3Asage_ir%3A%3Atypes');
  frame.parent=parent;
  return {frame,parent,fullPage};
}

test('embedded links preserve the book and opening a shared parent hash restores the selected entity',()=>{
  const selected='method:sage_ir::symbols::LocalFnSym::signature';
  const {frame,parent,fullPage}=frameFixture(`#${encodeURIComponent(selected)}`);
  const navigation=createNavigation(frame,model,book);
  assert.equal(navigation.host,parent);
  assert.equal(navigation.route(),selected);
  const target='item:sage_ir::types::Ty';
  assert.equal(navigation.href(target),`https://example.test/book/${bookHref(model,book,target)}`);
  assert.match(navigation.link(target,'Ty'),/target="_parent"/);
  assert.match(createRenderer(model,navigation.link).render(selected),/href="https:\/\/example.test\/book\/design\/crates\/sage_ir\/types.html#/);
  navigation.updateFullPage();
  assert.equal(fullPage.href,`https://example.test/book/sage-explorer.html#${encodeURIComponent(selected)}`);
});

test('parent hash changes and history restoration keep embedded selection and full-page links in agreement',()=>{
  const {frame,parent,fullPage}=frameFixture();
  const navigation=createNavigation(frame,model,book), observed=[];
  navigation.host.addEventListener('hashchange',()=>{observed.push(navigation.route());navigation.updateFullPage();});
  assert.equal(navigation.route(),'module:sage_ir::types');
  // These are the same location/hashchange transitions delivered by links and
  // browser Back/Forward; the iframe never creates a competing history entry.
  for(const route of ['item:sage_ir::types::Ty','item:sage_ir::types::FnSig','item:sage_ir::types::Ty','item:sage_ir::types::FnSig']) {
    parent.location.hash=encodeURIComponent(route);
    parent.dispatchEvent(new Event('hashchange'));
    assert.equal(navigation.route(),route);
    assert.equal(new URL(fullPage.href).hash,`#${encodeURIComponent(route)}`);
  }
  assert.deepEqual(observed,['item:sage_ir::types::Ty','item:sage_ir::types::FnSig','item:sage_ir::types::Ty','item:sage_ir::types::FnSig']);
  assert.equal(frame.location.hash,'');
});

test('parent chapter headings retain the default selection while unknown model entities remain errors',()=>{
  const {frame,parent,fullPage}=frameFixture('#types');
  const navigation=createNavigation(frame,model,book);
  for(const heading of ['#crates','#sage_ir','#types']) {
    parent.location.hash=heading;
    assert.equal(navigation.route(),'module:sage_ir::types');
    assert.equal(parent.location.hash,heading);
    navigation.updateFullPage();
    assert.equal(new URL(fullPage.href).hash,'#module%3Asage_ir%3A%3Atypes');
  }
  for(const selected of ['home','item:missing','field:sage_ir::types::Ty::missing']) {
    parent.location.hash=encodeURIComponent(selected);
    assert.equal(navigation.route(),selected);
  }
  assert.match(renderer.render(navigation.route()),/Not found/);
  parent.location.hash=encodeURIComponent('item:sage_ir::types::Ty');
  assert.equal(navigation.route(),'item:sage_ir::types::Ty');
});

test('standalone and restricted file-frame navigation remain functional without parent access',()=>{
  const frame=new EventTarget();
  frame.location=new URL('file:///tmp/book/sage-explorer.html?embed=1&route=crate%3Asage_ir');
  frame.parent={get location(){throw new Error('opaque origin');}};
  const navigation=createNavigation(frame,model,book);
  assert.equal(navigation.embedded,false);
  assert.equal(navigation.route(),'crate:sage_ir');
  assert.equal(navigation.href('home'),'#home');
  frame.location.hash=encodeURIComponent('item:sage_ir::types::Ty');
  assert.equal(navigation.route(),'item:sage_ir::types::Ty');
  frame.location.hash='%E0%A4%A';
  assert.equal(navigation.route(),'invalid');
  frame.parent=frame;
  assert.equal(createNavigation(frame,model,null).embedded,false);
});
