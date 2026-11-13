// Closed deployment drawers: geometry mirrors, portraits and icons do not.
'use strict';
const UnitRoster=(()=>{
  function supplyIcon(team,key){
    const icon=document.getElementById(`supply-card-${team}`).querySelector('svg').cloneNode(true);
    icon.setAttribute('aria-hidden','true');
    const maskId=`roster-supply-cut-${team}-${key}`;
    icon.querySelector('mask').id=maskId;
    icon.querySelector('[mask]').setAttribute('mask',`url(#${maskId})`);
    return icon;
  }
  function geometry(techCount){
    const columns=Math.min(2,Math.max(1,techCount)),rows=Math.max(1,Math.ceil(techCount/columns)),width=122,height=72;
    // Keep the portrait and frame fixed, fitting two columns into the same
    // strip. Six choices use three rows of 20-unit icons.
    const techSize=Math.min(22,(64-2*(rows-1))/rows),pitch=24;
    const rowPitch=rows<=2?26:techSize+2;
    const start=(68+width)/2-pitch*(columns-1)/2,startY=height/2-rowPitch*(rows-1)/2;
    return {width,height,techSize,path:`M1 1H${width-1}V${height-1}H1Z`,
      techs:Array.from({length:techCount},(_,i)=>({x:start+pitch*(i%columns),y:startY+rowPitch*Math.floor(i/columns)}))};
  }
  function render(root,entries,assets,team,tooltip){
    root.replaceChildren();root.scrollTop=0;
    const width=Math.max(122,...entries.map(entry=>geometry(entry.techs.length).width));
    // Centre a six-unit scrollbar in the twenty-unit gap before the arsenal.
    root.style.width=(width+13)+"px";
    root.parentElement?.style.setProperty("--roster-width",(width+20)+"px");
    const hide=()=>{tooltip.hidden=true;};
    root.onscroll=hide;
    const sorted=[...entries].sort((a,b)=>(b.investment_supply??0)-(a.investment_supply??0)||(a.kind<b.kind?-1:a.kind>b.kind?1:0));
    for(const [index,entry] of sorted.entries()){
      const unit=assets.units[entry.kind],name=unit?.name||entry.kind.replaceAll('_',' ');
      const shape=geometry(entry.techs.length);
      const change=Number.isInteger(entry.count_before)?entry.count-entry.count_before:0;
      const delta=change?` · ${change>0?'+':''}${change} this round`:'';
      const row=document.createElement('article');
      row.className='unit-drawer'+(change>0?' count-increased':change<0?' count-decreased':'');
      row.dataset.unitKind=entry.kind;
      row.style.width=shape.width+'px';
      row.style.setProperty('--tech-size',shape.techSize+'px');
      row.style.setProperty('--tech-padding',shape.techSize<22?'1px':'2px');
      row.style.height=shape.height+'px';row.setAttribute('role','listitem');
      row.setAttribute('aria-label',`${name}: ${entry.count} formations${delta}`);
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
      entry.techs.forEach(({id,active,activated_this_round,purchase_supply},i)=>{
        const status=active?(activated_this_round?'Activated this round':'Active'):'Inactive';
        const tech=assets.techs[id];const el=badge(shape.techs[i],'unit-tech '+(active?'active':'inactive')+(active&&activated_this_round?' activated':''));
        el.dataset.techId=String(id);
        el.tabIndex=0;el.setAttribute('role','img');el.setAttribute('aria-label',`${tech?.name||`Technology ${id}`} (${status})`);
        el.setAttribute('aria-describedby','roster-tooltip');
        if(tech && assets.images[tech.icon]){
          const icon=document.createElement('img');icon.src=assets.images[tech.icon];icon.alt='';icon.draggable=false;el.append(icon);
        }else el.textContent='?';
        const show=()=>{
          tooltip.replaceChildren();
          const title=document.createElement('b');title.textContent=`${tech?.name||`Technology ${id}`} · ${status}`;
          const effect=document.createElement('p');effect.textContent=tech?.effect||'Description unavailable';
          tooltip.append(title,effect);
          if(tech?.base_supply!==undefined){
            const price=document.createElement('div');price.className='tech-base-price';
            const known=Number.isInteger(purchase_supply),different=known&&purchase_supply!==tech.base_supply;
            price.setAttribute('aria-label',known?`${active?'Purchased for':'Purchase price'}: ${purchase_supply} supply${different?`, base ${tech.base_supply}`:''}`:`${tech.base_supply} supply; purchase price unavailable`);
            price.title=known?(active?'Historical purchase price':'Current purchase price'):'Base supply price; purchase history unavailable';
            // Reuse the HUD supply mark, including its SVG cutout.
            price.append(supplyIcon(team,'tooltip'));
            if(different){const base=document.createElement('s');base.textContent=String(tech.base_supply);price.append(base);}
            const value=document.createElement('span');value.textContent=String(known?purchase_supply:tech.base_supply);
            price.append(value);tooltip.append(price);
          }
          tooltip.hidden=false;
          const r=el.getBoundingClientRect(),box=tooltip.getBoundingClientRect();
          const x=team===0?r.right+10:r.left-box.width-10;
          tooltip.style.left=Math.max(8,Math.min(window.innerWidth-box.width-8,x))+'px';
          tooltip.style.top=Math.max(8,Math.min(window.innerHeight-box.height-8,r.top))+'px';
        };
        el.addEventListener('pointerenter',show);el.addEventListener('pointerleave',hide);
        el.addEventListener('focus',show);el.addEventListener('blur',hide);
      });
      const count=document.createElement('span');count.className='formation-count';
      count.textContent=String(entry.count);count.title=`${entry.count} formations${delta}`;
      row.append(count);
      const investment=document.createElement('span');investment.className='formation-investment';
      const supply=entry.investment_supply??0;
      investment.title=`${supply} supply invested: held formation values + researched technology costs`;
      investment.setAttribute('aria-label',investment.title);
      investment.append(supplyIcon(team,`investment-${index}`));
      const amount=document.createElement('span');amount.textContent=String(supply);investment.append(amount);
      row.append(investment);
      root.append(row);
      if(change){
        // Orbit the count badge, upright on both teams, rather than the card.
        const orbit=document.createElementNS('http://www.w3.org/2000/svg','svg');
        const width=count.offsetWidth,height=count.offsetHeight;
        orbit.classList.add('formation-orbit');orbit.setAttribute('viewBox',`0 0 ${width} ${height}`);orbit.setAttribute('aria-hidden','true');
        const w=width-1,h=height-1,n=Math.min(4,Math.abs(change));
        const path=change>0?`M1 1H${w}V${h}H1Z`:`M1 1V${h}H${w}V1Z`;
        // Match the skill contour's cool-white reflection: a bright head with
        // a fading short trail, following the border even around corners.
        orbit.innerHTML=Array.from({length:n},(_,i)=>Array.from({length:7},(_,j)=>{
          const offset=-100*i/n+j*1.6,opacity=0.94*(1-j/7)**1.5;
          return `<path d="${path}" pathLength="100" fill="none" stroke="#daf0ff" stroke-width="1.6" stroke-linecap="round" stroke-dasharray="2 98" opacity="${opacity}"><animate attributeName="stroke-dashoffset" from="${offset}" to="${offset-100}" dur="3s" repeatCount="indefinite" calcMode="linear"/></path>`;
        }).join('')).join('');
        count.append(orbit);
      }
    }
  }
  return {geometry,render};
})();
