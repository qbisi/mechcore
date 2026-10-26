// Closed deployment drawers: geometry mirrors, portraits and icons do not.
'use strict';
const UnitRoster=(()=>{
  function geometry(techCount){
    const columns=Math.max(1,Math.ceil(techCount/2)),width=Math.max(122,74+24*columns),height=72;
    const start=(68+width)/2-12*(columns-1);
    return {width,height,path:`M1 1H${width-1}V${height-1}H1Z`,
      techs:Array.from({length:techCount},(_,i)=>({x:start+24*(i%columns),y:23+26*Math.floor(i/columns)}))};
  }
  function render(root,entries,assets,team,tooltip){
    root.replaceChildren();root.scrollTop=0;
    const width=Math.max(122,...entries.map(entry=>geometry(entry.techs.length).width));
    root.style.width=(width+20)+"px";
    root.parentElement?.style.setProperty("--roster-width",(width+20)+"px");
    const hide=()=>{tooltip.hidden=true;};
    root.onscroll=hide;
    for(const [index,entry] of entries.entries()){
      const unit=assets.units[entry.kind],name=unit?.name||entry.kind.replaceAll('_',' ');
      const shape=geometry(entry.techs.length);
      const row=document.createElement('article');
      row.className='unit-drawer';
      row.style.width=shape.width+'px';
      row.style.height=shape.height+'px';row.setAttribute('role','listitem');
      row.setAttribute('aria-label',`${name}: ${entry.count} formations`);
      const frame=document.createElementNS('http://www.w3.org/2000/svg','svg');
      frame.setAttribute('viewBox',`0 0 ${shape.width} ${shape.height}`);frame.setAttribute('aria-hidden','true');
      frame.classList.add('drawer-shape');
      const id=`drawer-${team}-${index}`;
      frame.innerHTML=`<defs><linearGradient id="${id}" x2="1" y2="1"><stop stop-color="#243448" stop-opacity=".94"/><stop offset="1" stop-color="#0c141e" stop-opacity=".9"/></linearGradient></defs><path d="${shape.path}" fill="url(#${id})" stroke="#73849b" stroke-width="1.2"/><path d="M${shape.width-1} 1V${shape.height-1}" fill="none" stroke="${team===0?'#509ee3':'#d96d79'}" stroke-width="2"/>`;
      row.append(frame);
      if(unit && assets.images[unit.portrait]){
        const portrait=document.createElement('img');portrait.className='unit-portrait';
        portrait.src=assets.images[unit.portrait];portrait.alt=name;portrait.draggable=false;
        portrait.style.top=(shape.height/2-28)+'px';row.append(portrait);
      }
      function badge(point,className){
        const el=document.createElement('span');el.className='drawer-badge '+className;
        el.style[team===0?'left':'right']=point.x+'px';el.style.top=point.y+'px';
        row.append(el);return el;
      }
      entry.techs.forEach(({id,active},i)=>{
        const tech=assets.techs[id];const el=badge(shape.techs[i],'unit-tech '+(active?'active':'inactive'));
        el.tabIndex=0;el.setAttribute('role','img');el.setAttribute('aria-label',`${tech?.name||`Technology ${id}`} (${active?'Active':'Inactive'})`);
        el.setAttribute('aria-describedby','roster-tooltip');
        if(tech && assets.images[tech.icon]){
          const icon=document.createElement('img');icon.src=assets.images[tech.icon];icon.alt='';icon.draggable=false;el.append(icon);
        }else el.textContent='?';
        const show=()=>{
          tooltip.replaceChildren();
          const title=document.createElement('b');title.textContent=`${tech?.name||`Technology ${id}`} · ${active?'Active':'Inactive'}`;
          const effect=document.createElement('p');effect.textContent=tech?.effect||'Description unavailable';
          tooltip.append(title,effect);tooltip.hidden=false;
          const r=el.getBoundingClientRect(),box=tooltip.getBoundingClientRect();
          const x=team===0?r.right+10:r.left-box.width-10;
          tooltip.style.left=Math.max(8,Math.min(window.innerWidth-box.width-8,x))+'px';
          tooltip.style.top=Math.max(8,Math.min(window.innerHeight-box.height-8,r.top))+'px';
        };
        el.addEventListener('pointerenter',show);el.addEventListener('pointerleave',hide);
        el.addEventListener('focus',show);el.addEventListener('blur',hide);
      });
      const count=document.createElement('span');count.className='formation-count';
      count.textContent=String(entry.count);count.title=`${entry.count} formations`;
      row.append(count);
      root.append(row);
    }
  }
  return {geometry,render};
})();
