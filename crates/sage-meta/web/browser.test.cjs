const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const {resolve} = require('node:path');
const {buildModel,createRenderer,searchModel,sourceOwner,renderSourcePane} = require('./browser.js');
const artifact = resolve(__dirname, '../../../target/sage-browser.html');
const html = readFileSync(artifact,'utf8');
const files = JSON.parse(html.split('<script id="model-data" type="application/json">')[1].split('</script>')[0]);
const model = buildModel(files), renderer = createRenderer(model);

test('indexes actual crates, types, methods, and Rust annotations',()=>{
  assert.ok(model.items.size > 0);
  assert.ok(model.crates.has('sage_ir'));
  assert.equal(model.items.get('sage_ir::types::Ty').kind,'enum');
  assert.equal(model.items.get('sage_ir::symbols::LocalFnSym').rust.target.Type.kind.Named.path.segments.at(-1),'LocalFnSym');
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
  const routes=['home',...[...model.crates.keys()].map(k=>`crate:${k}`),...[...model.modules.keys()].map(k=>`module:${k}`),...[...model.items.keys()].map(k=>`item:${k}`),...[...model.methods.keys()].map(k=>`method:${k}`)];
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
  assert.match(renderer.render('item:sage_ir::types::Signature'),/review abstraction/);
});

test('escapes HTML in names, prose, and Rust paths',()=>{
  const hostile=structuredClone(files);
  const ty=hostile.find(f=>f.namespace==='sage_ir::types').document.declarations.find(d=>d.Type?.name==='Ty').Type;
  ty.description='<img src=x onerror=alert(1)>';
  ty.rust.target.Type.kind.Named.path.root='<script>';
  const rendered=createRenderer(buildModel(hostile)).render('item:sage_ir::types::Ty');
  assert.ok(!rendered.includes('<img'));
  assert.ok(!rendered.includes('<script>'));
  assert.match(rendered,/&lt;img/);
});


test('searches names and Rust paths and filters by crate and operation kind',()=>{
  const rust=searchModel(model,'crate::ty::FnSig');
  assert.ok(rust.some(item=>item.key==='sage_ir::types::FnSig'));
  const queries=searchModel(model,'signature','sage_ir','query');
  assert.ok(queries.length>0);
  assert.ok(queries.every(item=>item.crateName==='sage_ir'&&item.kind==='query'));
  assert.equal(searchModel(model,'no_such_declaration').length,0);
});

const sources=JSON.parse(html.split('<script id="source-data" type="application/json">')[1].split('</script>')[0]);

test('opens annotated Rust definitions and follows method owners',()=>{
  const item=model.items.get('sage_ir::types::Ty');
  const refs=sources.targets[item.key];
  assert.ok(refs[0].locations.length);
  const location=refs[0].locations[0];
  assert.match(sources.files[location.file].split('\n')[location.line-1], /enum Ty/);
  assert.match(renderer.render('item:'+item.key),/data-source="sage_ir::types::Ty"/);
  assert.match(renderSourcePane(item,sources),/data-definition="true"/);
  assert.equal(sourceOwner(model,'method:sage_ir::symbols::LocalFnSym::body').name,'LocalFnSym');
  assert.equal(sourceOwner(model,'home'),null);
});

test('resolves workspace annotations, labels macro inputs, and explains abstractions',()=>{
  for(const [key,refs] of Object.entries(sources.targets)){
    if(['sage_ir::types::Boolean','sage_ir::types::Unit'].includes(key))continue;
    assert.ok(refs.some(ref=>ref.locations.length),key);
  }
  assert.match(renderSourcePane(model.items.get('sage_ir::symbols::Symbol'),sources),/Macro input/);
  assert.match(renderSourcePane(model.items.get('sage_ir::types::Signature'),sources),/review abstraction/);
  assert.match(renderSourcePane(model.items.get('sage_ir::types::Boolean'),sources),/No workspace source/);
});

test('escapes Rust source and supports choosing among definitions',()=>{
  const item=model.items.get('sage_ir::types::Ty');
  const fixture={files:{'a.rs':'<script>alert(1)</script>','b.rs':'enum Other {}'},targets:{[item.key]:[{path:'crate::Ty',locations:[
    {file:'a.rs',line:1,end_line:1,macro_input:false},
    {file:'b.rs',line:1,end_line:1,macro_input:false}
  ]}]}};
  const first=renderSourcePane(item,fixture);
  assert.ok(!first.includes('<script>'));
  assert.match(first,/&lt;script&gt;/);
  assert.match(renderSourcePane(item,fixture,'0:1'),/enum Other/);
});
