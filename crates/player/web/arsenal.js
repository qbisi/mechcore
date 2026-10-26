// Read-only deployment inventory. Cooldowns are rounds, never combat seconds.
'use strict';
const ArsenalPanel=(()=>{
  function render(root,data,assets,team,tooltip){
    root.replaceChildren();
    const columns=Number(data.skills.length>0)+Number(data.equipment.length>0);
    const width=columns?44*columns+8*(columns-1):0;
    root.style.width=width+'px';
    if(root.parentElement){
      root.parentElement.style.setProperty('--arsenal-width',width+'px');
      root.parentElement.hidden=columns===0;
    }
    for(const [group,entries] of [['skills',data.skills],['equipment',data.equipment]]){
      if(!entries.length)continue;
      const list=document.createElement('section');list.className='arsenal-list '+group;
      list.setAttribute('aria-label',group==='skills'?'Battlefield skills':'Unequipped items');
      list.onscroll=()=>{tooltip.hidden=true;};
      for(const entry of entries){
        const asset=assets[group][entry.id];
        const skill=group==='skills';
        const state=skill?(entry.released?'deployed':entry.used?'used':entry.cooldown>0?'cooling':'ready'):'inventory';
        const status=skill?(entry.released?'Deployed this round':entry.used?'Used this round':entry.cooldown>0?`Cooldown: ${entry.cooldown} rounds`:'Available'):
          `Unequipped${entry.durability!==null&&entry.durability>=0?` · Durability: ${entry.durability}`:''}`;
        const name=asset?.name||String(entry.id);
        const card=document.createElement('button');card.type='button';card.className='arsenal-card '+state;
        card.setAttribute('aria-label',`${name}: ${status}`);
        if(asset&&assets.images[asset.icon]){
          const icon=document.createElement('img');icon.src=assets.images[asset.icon];icon.alt='';icon.draggable=false;card.append(icon);
        }else card.textContent='?';
        const label=skill?(state==='cooling'?String(entry.cooldown):entry.released||entry.used?'✓':''):'';
        if(label){const badge=document.createElement('span');badge.className='arsenal-badge';badge.textContent=label;card.append(badge);}
        const hide=()=>{tooltip.hidden=true;};
        const show=()=>{
          tooltip.replaceChildren();
          const title=document.createElement('b');title.textContent=name;
          const detail=document.createElement('p');detail.textContent=status;
          const effect=document.createElement('p');effect.textContent=asset?.effect||'Description unavailable';
          tooltip.append(title,detail,effect);tooltip.hidden=false;
          const r=card.getBoundingClientRect(),box=tooltip.getBoundingClientRect();
          const x=team===0?r.right+10:r.left-box.width-10;
          tooltip.style.left=Math.max(8,Math.min(window.innerWidth-box.width-8,x))+'px';
          tooltip.style.top=Math.max(8,Math.min(window.innerHeight-box.height-8,r.top))+'px';
        };
        card.addEventListener('pointerenter',show);card.addEventListener('pointerleave',hide);
        card.addEventListener('focus',show);card.addEventListener('blur',hide);
        list.append(card);
      }
      root.append(list);
    }
  }
  return {render};
})();
