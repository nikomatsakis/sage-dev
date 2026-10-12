/* The explorer consumes the parsed AST; it never interprets prose or runs .sage. */
(function () {
  'use strict';
  const escape = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const tag = value => typeof value === 'string' ? value : Object.keys(value)[0];
  const payload = value => typeof value === 'string' ? null : value[tag(value)];
  const link = (id, label) => `<a href="#${encodeURIComponent(id)}">${escape(label)}</a>`;
  const prose = text => text ? `<div class="prose">${escape(text.trim())}</div>` : '';
  const badge = text => `<span class="badge">${escape(text)}</span>`;

  function buildModel(files, sources={targets:{},files:{},warnings:[]}) {
    const items = new Map(), methods = new Map(), crates = new Map(), modules = new Map();
    for (const file of files) {
      const {namespace, document: doc} = file;
      const crateName = namespace.split('::')[0];
      if (doc.header.Crate) crates.set(crateName, {...doc.header.Crate, key:crateName, namespace, file: doc.file});
      modules.set(namespace, {key:namespace, name:namespace, namespace, crateName, file: doc.file, declarations: []});
      for (const declaration of doc.declarations) {
        const value = declaration.Type || declaration.Class;
        const kind = declaration.Type ? ({Opaque:'opaque',Struct:'struct',Enum:'enum'}[tag(value.shape)]) : (value.kind === 'Context' ? 'context' : 'class');
        const key = `${namespace}::${value.name}`;
        const item = {...value, key, namespace, crateName, file: doc.file, kind, isClass: !!declaration.Class, abstract: value.kind === 'Abstract'};
        items.set(key, item);
        modules.get(namespace).declarations.push(item);
        for (const method of value.methods || []) methods.set(`${key}::${method.name}`, {...method, key:`${key}::${method.name}`, owner:key, namespace, crateName, kind:method.kind === 'Query' ? 'query' : 'fn'});
      }
    }
    function pathKey(path, namespace, owner) {
      if (path.root === 'self') return `${owner}::${path.segments.join('::')}`;
      const root = path.root === 'crate' ? namespace.split('::')[0] : path.root.startsWith('$') ? path.root.slice(1) : `${namespace}::${path.root}`;
      return [root, ...path.segments].join('::');
    }
    function ancestors(key) {
      const result = [];
      while (key && !result.includes(key)) {
        result.push(key);
        const item = items.get(key);
        key = item?.base ? pathKey(item.base, item.namespace) : null;
      }
      return result;
    }
    function methodKey(key) {
      if (methods.has(key)) return key;
      const cut = key.lastIndexOf('::'), owner = key.slice(0,cut), name = key.slice(cut+2);
      return ancestors(owner).map(base => `${base}::${name}`).find(candidate => methods.has(candidate));
    }
    function callTargets(path, method) {
      const key = pathKey(path, method.namespace, method.owner);
      const cut = key.lastIndexOf('::'), owner = key.slice(0,cut), name = key.slice(cut+2);
      const targets = new Set();
      for (const candidate of items.values()) if (candidate.isClass && ancestors(candidate.key).includes(owner)) {
        const target = methodKey(`${candidate.key}::${name}`);
        if (target) targets.add(target);
      }
      return [...targets].sort();
    }
    const calls = new Map(), incoming = new Map();
    for (const method of methods.values()) {
      const targets = new Set();
      for (const effect of method.envelope || []) if (effect.Invokes) for (const key of callTargets(effect.Invokes, method)) targets.add(key);
      calls.set(method.key, [...targets].sort());
      for (const key of targets) {
        if (!incoming.has(key)) incoming.set(key, []);
        incoming.get(key).push(method.key);
      }
    }
    return {sources, items, methods, crates, modules, pathKey, ancestors, methodKey, callTargets, calls, incoming};
  }

  function createRenderer(model) {
    function typeText(type) {
      if (type.kind.Tuple) return `(${type.kind.Tuple.map(typeText).join(', ')})`;
      const {path, arguments:args} = type.kind.Named;
      return [path.root,...path.segments].join('::') + (args.length ? `<${args.map(typeText).join(', ')}>` : '');
    }
    function typeHtml(type, item, parameters = []) {
      if (type.kind.Tuple) return `(${type.kind.Tuple.map(t => typeHtml(t,item,parameters)).join(', ')})`;
      const {path, arguments:args} = type.kind.Named;
      const key = model.pathKey(path, item.namespace);
      const label = [path.root,...path.segments].join('::');
      const name = !parameters.includes(path.root) && model.items.has(key) ? link(`item:${key}`, label) : escape(label);
      return name + (args.length ? `&lt;${args.map(t=>typeHtml(t,item,parameters)).join(', ')}&gt;` : '');
    }
    function origin(item) {
      const sites=model.sources.targets[item.key]||[];
      if(!sites.length) return '<p class="section-note">No Rust correspondence in this snapshot.</p>';
      return `<div class="origin"><button class="source-link" data-source="${escape(item.key)}"><code>${escape(sites[0].path)}</code></button><span class="muted">${sites.length} source ${sites.length===1?'site':'sites'}</span></div>`;
    }
    function fields(values,item) {
      if (!values.length) return '<p class="section-note">No fields.</p>';
      return `<table><thead><tr><th>Field</th><th>Type</th><th>Description</th></tr></thead><tbody>${values.map(f=>`<tr><td><code>${escape(f.name)}</code>${f.kind ? ` ${badge(f.kind.toLowerCase())}`:''}</td><td><code>${typeHtml(f.ty,item,item.parameters||[])}</code></td><td>${prose(f.description)}</td></tr>`).join('')}</tbody></table>`;
    }
    function methodLink(key) { return link(`method:${key}`,key); }
    function effectHtml(effect, method) {
      const name = tag(effect), data = payload(effect);
      if (name === 'Creates') {
        const key = model.pathKey(data.context,method.namespace);
        return `<li><span class="label">creates</span><code>${escape(data.name)}: ${link(`item:${key}`,key)}</code></li>`;
      }
      if (name === 'Reads') return `<li><span class="label">reads</span><code>self.${escape(data.segments.join('::'))}</code></li>`;
      const key = model.pathKey(data,method.namespace,method.owner);
      const resolved = model.methodKey(key);
      const label = data.root === 'self' ? `self.${data.segments.join('::')}()` : `${key}()`;
      return `<li><span class="label ${name==='ForbidTransitive'?'forbid':''}">${name==='Invokes'?'invokes':'forbid transitive'}</span><code>${resolved ? link(`method:${resolved}`,label) : escape(label)}</code></li>`;
    }
    function methodCard(method, detail=false) {
      const owner = model.items.get(method.owner);
      return `<section class="method"><div class="method-title">${badge(method.kind)}<code>${link(`method:${method.key}`,`${method.name}()`)} → ${typeHtml(method.result,owner)}</code>${method.is_override?badge('override'):''}${method.envelope===null?badge('abstract'):''}</div>${prose(method.description)}${detail ? `<h3>Dependency envelope</h3>${method.envelope===null ? '<p class="section-note">Abstract declaration; follow its implementations below.</p>' : `<ul class="effect-list">${method.envelope.map(e=>effectHtml(e,method)).join('')}</ul>${method.envelope.length===0?'<p class="section-note">A leaf computation in this model.</p>':''}`}` : ''}</section>`;
    }
    function breadcrumbs(item) {
      return `<div class="breadcrumb">${link('home','Architecture')} / ${link(`crate:${item.crateName}`,item.crateName)} / ${link(`module:${item.namespace}`,item.namespace.split('::').slice(1).join('::') || 'crate')}</div>`;
    }
    function itemCard(item) {
      return `<div class="card">${badge(item.kind)}<h3 style="margin-top:12px">${link(`item:${item.key}`,item.name)}</h3>${prose((item.description||'').trim().split('\n')[0])}</div>`;
    }
    function overview() {
      const featured=['sage_ir::symbols::LocalFnSym','sage_ir::types::Ty','sage_ir::inference::InferCtx','sage_ir::solver::Solver'].map(k=>model.items.get(k)).filter(Boolean);
      return `<div class="eyebrow">The compiler, one computation at a time</div><h1>Explore the architecture</h1><div class="prose">Follow a symbol to its methods, a method to the interfaces it uses, and a type to the values it contains.</div><div class="stats"><div><strong>${model.crates.size}</strong><span>crates</span></div><div><strong>${model.items.size}</strong><span>declarations</span></div><div><strong>${model.methods.size}</strong><span>methods & queries</span></div></div>${featured.length?`<h2>Start exploring</h2><div class="grid">${featured.map(itemCard).join('')}</div>`:''}<h2>Crates</h2><div class="grid">${[...model.crates.values()].map(c=>`<div class="card"><h3>${link(`crate:${c.name}`,c.name)}</h3>${prose(c.purpose.split('\n')[0])}<p class="section-note">${c.modules.length} logical modules · ${c.dependencies.length} dependencies</p></div>`).join('')}</div>`;
    }
    function render(route) {
      if (!route || route==='home') return overview();
      const cut=route.indexOf(':'), kind=route.slice(0,cut), key=route.slice(cut+1);
      if (kind==='crate' && model.crates.has(key)) {
        const c=model.crates.get(key);
        const own=[...model.items.values()].filter(i=>i.namespace===key);
        return `<div class="breadcrumb">${link('home','Architecture')} / ${escape(key)}</div><div class="eyebrow">Crate</div><h1>${escape(key)}</h1>${prose(c.purpose)}${origin(c)}<h2>Workspace dependencies</h2><p>${c.dependencies.map(d=>link(`crate:${d}`,d)).join(' · ') || 'None declared.'}</p><h2>Logical modules</h2><div class="grid">${c.modules.map(name=>`<div class="card"><h3>${link(`module:${key}::${name}`,name)}</h3><span class="muted">${model.modules.get(`${key}::${name}`)?.declarations.length||0} declarations</span></div>`).join('') || '<p class="section-note">This crate is described at the crate level.</p>'}</div>${own.length?`<h2>Declarations</h2><div class="grid">${own.map(itemCard).join('')}</div>`:''}<p class="file">${escape(c.file)}</p>`;
      }
      if (kind==='module' && model.modules.has(key)) {
        const mod=model.modules.get(key);
        return `${breadcrumbs(mod)}<div class="eyebrow">Module</div><h1>${escape(key)}</h1>${origin(mod)}<div class="grid">${mod.declarations.map(itemCard).join('')}</div><p class="file">${escape(mod.file)}</p>`;
      }
      if (kind==='item' && model.items.has(key)) {
        const item=model.items.get(key);
        let content='';
        if (item.isClass) {
          if (item.base) { const base=model.pathKey(item.base,item.namespace); content+=`<p>Extends ${link(`item:${base}`,base)}</p>`; }
          const subclasses=[...model.items.values()].filter(i=>i.base && model.pathKey(i.base,i.namespace)===key);
          if(subclasses.length) content+=`<p>Subclasses: ${subclasses.map(i=>link(`item:${i.key}`,i.name)).join(' · ')}</p>`;
          content+=`<h2>Fields</h2>${fields(item.fields,item)}<h2>Methods</h2>${item.methods.map(m=>methodCard(model.methods.get(`${key}::${m.name}`))).join('') || '<p class="section-note">No methods declared here.</p>'}`;
          const selected=new Set(item.methods.map(m=>m.name)), inherited=[];
          for(const ancestor of model.ancestors(key).slice(1)) for(const method of model.items.get(ancestor)?.methods||[]) if(!selected.has(method.name)){selected.add(method.name);inherited.push(`${ancestor}::${method.name}`);}
          if(inherited.length) content+=`<h2>Inherited methods</h2><ul class="call-list">${inherited.map(k=>`<li>${methodLink(k)}</li>`).join('')}</ul>`;
        } else if(item.kind==='struct') content=`<h2>Fields</h2>${fields(item.shape.Struct,item)}`;
        else if(item.kind==='enum') content=`<h2>Variants</h2>${item.shape.Enum.map(v=>`<details open><summary>${escape(v.name)}${v.fields.Tuple ? `(${v.fields.Tuple.map(t=>typeHtml(t,item,item.parameters)).join(', ')})`:''}</summary>${prose(v.description)}${v.fields.Named?fields(v.fields.Named,item):''}</details>`).join('')}`;
        else content='<p class="section-note">Opaque by design. Its meaning is described above; its representation is left unspecified.</p>';
        return `${breadcrumbs(item)}<div class="eyebrow">${item.abstract?'abstract ':''}${escape(item.kind)}</div><h1>${escape(item.name)}${item.parameters?.length?`&lt;${item.parameters.map(escape).join(', ')}&gt;`:''}</h1>${prose(item.description)}${origin(item)}${content}<p class="file">${escape(item.file)} · byte ${item.span.start}</p>`;
      }
      if(kind==='method' && model.methods.has(key)) {
        const method=model.methods.get(key), owner=model.items.get(method.owner);
        const targets=model.calls.get(key)||[], incoming=model.incoming.get(key)||[];
        const inheritedRules=model.ancestors(method.owner).slice(1).flatMap(base=>{
          const inherited=model.methods.get(`${base}::${method.name}`);
          return inherited ? (inherited.envelope||[]).filter(effect=>effect.ForbidTransitive).map(effect=>({inherited,effect})) : [];
        });
        const implementations=[...model.items.values()].filter(i=>i.isClass && model.ancestors(i.key).includes(method.owner)).map(i=>model.methodKey(`${i.key}::${method.name}`)).filter(k=>k&&k!==key);
        const list=keys=>keys.length?`<ul class="call-list">${[...new Set(keys)].map(k=>`<li>${methodLink(k)}</li>`).join('')}</ul>`:'<p class="section-note">None in the declared model.</p>';
        return `${breadcrumbs(owner)}<div class="eyebrow">${escape(method.kind)}</div><h1>${escape(owner.name)}::${escape(method.name)}</h1><p>Defined on ${link(`item:${owner.key}`,owner.name)}</p>${origin(method)}${methodCard(method,true)}${inheritedRules.length?`<h2>Inherited prohibitions</h2><ul class="effect-list">${inheritedRules.map(({inherited,effect})=>effectHtml(effect,inherited)).join('')}</ul>`:''}<div class="grid"><section><h2>Possible direct calls</h2><p class="section-note">Includes known subclass dispatch. Permissions do not imply execution.</p>${list(targets)}</section><section><h2>Called by</h2><p class="section-note">Methods whose declared calls can reach this implementation directly.</p>${list(incoming)}</section></div>${implementations.length?`<h2>Overriding implementations</h2>${list(implementations)}`:''}<p class="file">${escape(owner.file)} · byte ${method.span.start}</p>`;
      }
      return `<h1>Not found</h1><p class="prose">This link does not name a declaration in this snapshot.</p>${link('home','Back to overview')}`;
    }
    return {render,typeText};
  }

  function searchModel(model, query='', crate='', kind='') {
    const q=query.toLowerCase().trim();
    return [...model.items.values(),...model.methods.values()].filter(i=>(!crate||i.crateName===crate)&&(!kind||i.kind===kind)&&(!q||`${i.key} ${i.description} ${(model.sources.targets[i.key]||[]).map(s=>s.path).join(' ')}`.toLowerCase().includes(q)));
  }
  function sourceOwner(model, route) {
    if(route.startsWith('crate:')) return model.crates.get(route.slice(6));
    if(route.startsWith('module:')) return model.modules.get(route.slice(7));
    if(route.startsWith('item:')) return model.items.get(route.slice(5));
    if(route.startsWith('method:')) return model.methods.get(route.slice(7));
    return null;
  }
  function renderSourcePane(item, sources, choice='') {
    if(!item) return '<div class="source-empty">Choose a declaration or method to explore its Rust source.</div>';
    const references=sources.targets[item.key]||[];
    const options=references.flatMap((reference,r)=>reference.locations.map((location,l)=>({reference,location,value:`${r}:${l}`})));
    if(!options.length) return `<div class="source-empty"><h3>${escape(item.name)}</h3><p>No workspace source is embedded for this annotation.</p><p class="section-note">Regenerate from the Cargo workspace to include annotated Rust definitions and marker modules.</p>${sources.warnings?.length?`<details><summary>Source indexing notes</summary>${prose(sources.warnings.join('\n'))}</details>`:''}</div>`;
    const selected=options.find(option=>option.value===choice)||options[0];
    const {reference,location}=selected;
    const text=sources.files[location.file];
    if(typeof text!=='string') return '<div class="source-empty">The source file is missing from this snapshot.</div>';
    const unavailable=references.filter(reference=>!reference.locations.length);
    return `<div class="source-meta"><label for="source-location">${escape(item.name)} · definition</label><select id="source-location" aria-label="Select Rust source definition">${options.map(option=>`<option value="${option.value}" ${option.value===selected.value?'selected':''}>${escape(option.reference.path)} — ${escape(option.location.file)}:${option.location.line}</option>`).join('')}</select><div class="source-file"><code>${escape(location.file)}:${location.line}</code><button data-copy="${escape(location.file)}:${location.line}">Copy location</button></div>${location.kind==='Module'?'<p class="section-note">Module correspondence anchor; this does not establish executable coverage.</p>':''}${location.macro_input?'<p class="section-note">Macro input declaring this type; expanded Rust is not included.</p>':''}${options.length>1?'<p class="section-note">Choose among the Rust correspondence sites above. Source indexing does not evaluate cfg.</p>':''}${unavailable.length?`<p class="section-note">No workspace definition: ${unavailable.map(r=>escape(r.path)).join(', ')}.</p>`:''}<span class="source-snapshot">Source snapshot captured when this HTML was generated.</span></div><div class="source-code" tabindex="0" aria-label="Rust source code">${text.split('\n').map((line,index)=>`<div class="source-line ${index+1>=location.line&&index+1<=location.end_line?'highlight':''}" ${index+1===location.line?'data-definition="true"':''}><span class="line-number" aria-hidden="true">${index+1}</span><code>${escape(line)||' '}</code></div>`).join('')}</div>`;
  }
  const api={buildModel,createRenderer,searchModel,sourceOwner,renderSourcePane,escape};
  if(typeof module!=='undefined' && module.exports) module.exports=api;
  if(typeof document==='undefined') return;
  const sources=JSON.parse(document.getElementById('source-data').textContent);
  const model=buildModel(JSON.parse(document.getElementById('model-data').textContent),sources);
  const renderer=createRenderer(model), search=document.getElementById('search'), crateFilter=document.getElementById('crate-filter'), kindFilter=document.getElementById('kind-filter');
  for(const name of model.crates.keys()){const option=document.createElement('option');option.value=name;option.textContent=name;crateFilter.append(option);}
  function route(){try{return decodeURIComponent(location.hash.slice(1))||'home';}catch{return 'invalid';}}
  function navigation(){
    const q=search.value.toLowerCase().trim(), crate=crateFilter.value, kind=kindFilter.value;
    const filtered=searchModel(model,q,crate,kind);
    document.getElementById('result-count').textContent=`${filtered.length} matching declarations & methods`;
    let html='';
    for(const name of model.crates.keys()){
      const entries=filtered.filter(i=>i.crateName===name);
      if(!entries.length && (q||kind||crate&&crate!==name))continue;
      html+=`<div class="nav-crate">${link(`crate:${name}`,name)}</div>`;
      const namespaces=[...new Set(entries.map(i=>i.namespace))];
      for(const namespace of namespaces){
        html+=`<a class="nav-module" href="#${encodeURIComponent(`module:${namespace}`)}">${escape(namespace.split('::').slice(1).join('::')||'crate')}</a>`;
        for(const item of entries.filter(i=>i.namespace===namespace)){
          const id=`${item.owner?'method':'item'}:${item.key}`;
          html+=`<a class="nav-item ${route()===id?'active':''}" ${route()===id?'aria-current="page"':''} href="#${encodeURIComponent(id)}"><span class="dot"></span>${escape(item.owner?`${model.items.get(item.owner).name}::${item.name}()`:item.name)}</a>`;
        }
      }
    }
    document.getElementById('navigation').innerHTML=html||'<div class="empty">No matches. Try a shorter name or clear the filters.</div>';
  }
  let sourceOpen=false;
  let selectedSource=null;
  function refreshSource(choice='') {
    const item=model.items.get(selectedSource)||model.methods.get(selectedSource)||model.crates.get(selectedSource)||model.modules.get(selectedSource)||sourceOwner(model,route());
    document.getElementById('source-content').innerHTML=renderSourcePane(item,sources,choice);
    requestAnimationFrame(()=>{
      const row=document.querySelector('[data-definition]');
      const code=document.querySelector('.source-code');
      if(row&&code) code.scrollTop=row.offsetTop-code.clientHeight/3;
    });
  }
  function toggleSource(open) {
    sourceOpen=open;
    document.querySelector('.layout').classList.toggle('source-open',open);
    document.getElementById('rust-source').hidden=!open;
    document.getElementById('source-toggle').setAttribute('aria-expanded',String(open));
    if(open) refreshSource();
  }
  document.getElementById('source-toggle').addEventListener('click',()=>toggleSource(!sourceOpen));
  document.getElementById('source-close').addEventListener('click',()=>{toggleSource(false);document.getElementById('source-toggle').focus();});
  document.addEventListener('change',event=>{if(event.target.id==='source-location')refreshSource(event.target.value);});
  document.addEventListener('click',event=>{const button=event.target.closest('[data-source]');if(button){selectedSource=button.dataset.source;toggleSource(true);document.getElementById('source-location')?.focus();}});
  function show(){selectedSource=null;if(sourceOpen)refreshSource();const main=document.getElementById('content');main.innerHTML=renderer.render(route());main.scrollTop=0;navigation();document.title=`${route()==='home'?'Architecture':route().split(':').slice(1).join(':')} · Sage`;}
  search.addEventListener('input',navigation);crateFilter.addEventListener('change',navigation);kindFilter.addEventListener('change',navigation);
  window.addEventListener('hashchange',show);
  document.querySelector('.skip').addEventListener('click',event=>{event.preventDefault();document.getElementById('content').focus();});
  document.addEventListener('keydown',event=>{if(event.key==='/'&&!['INPUT','TEXTAREA','SELECT'].includes(document.activeElement.tagName)){event.preventDefault();search.focus();}if(event.key==='Escape'&&document.activeElement===search){search.value='';navigation();}});
  document.getElementById('theme').addEventListener('click',()=>{document.body.classList.toggle('dark');});
  document.addEventListener('click',async event=>{const button=event.target.closest('[data-copy]');if(!button)return;try{await navigator.clipboard.writeText(button.dataset.copy);button.textContent='Copied';}catch{button.textContent='Select path to copy';}});
  show();
})();
