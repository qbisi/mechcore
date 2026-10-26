// Original vector geometry inspired by the native HUD, in CSS pixels.
// End caps, rails and texture pitch stay fixed while the middle gets longer.
'use strict';
const HudFrame = (() => {
  // The design's CSS dimensions at 1.25 pixels per world metre. Scale the
  // entire HUD with the natural map fit, independently of camera zoom.
  const uiScale=naturalFit=>naturalFit/1.25;
  function outline(width) {
    const w=Math.max(80,width), plate=Math.min(w-42,Math.max(110,w*.64));
    return {width:w,plate,
      outer:`M8 1H${w-1}V18L${w-13} 30H${plate+16}L${plate-6} 53H1V8Z`,
      inset:`M9 4H${w-4}V17L${w-15} 27H7V8Z`,
      separator:`M7 30H${plate+16}`,
      plateRail:`M7 50H${plate-8}L${plate+9} 33`,
      topRail:`M12 3H${w-6}`,
    };
  }
  function border(width,id) {
    const g=outline(width);
    return `<defs>
      <linearGradient id="${id}-metal" x2="0" y2="1"><stop stop-color="#c4cbd5"/><stop offset=".17" stop-color="#5d6a7d"/><stop offset=".56" stop-color="#28364b"/><stop offset="1" stop-color="#7286a3"/></linearGradient>
      <linearGradient id="${id}-panel" x2="0" y2="1"><stop stop-color="#202a3a" stop-opacity=".94"/><stop offset=".58" stop-color="#101824" stop-opacity=".87"/><stop offset="1" stop-color="#27364d" stop-opacity=".9"/></linearGradient>
    </defs>
    <path d="${g.outer}" fill="url(#${id}-panel)" stroke="url(#${id}-metal)" stroke-width="1.4"/>
    <path d="${g.inset}" fill="#090f19" fill-opacity=".5" stroke="#41526b" stroke-opacity=".8" stroke-width=".7"/>
    <path d="${g.separator}" fill="none" stroke="#060c15" stroke-opacity=".75"/>
    <path d="${g.plateRail}" fill="none" stroke="#6b84ac" stroke-opacity=".55" stroke-width="1"/>
    <path d="${g.topRail}" fill="none" stroke="#d5e1ed" stroke-opacity=".24" stroke-width=".6"/>`;
  }
  function center(width) {
    const w=Math.max(80,width);
    return `<path d="M1 1H${w-1}L${w-12} 24H${w*.7}L${w*.7-5} 28H${w*.3+5}L${w*.3} 24H12Z" fill="#131c29" fill-opacity=".78" stroke="#64748b" stroke-opacity=".45"/>
      <path d="M7 3H${w-7}" stroke="#9aabc1" stroke-opacity=".18"/>`;
  }
  return {outline,border,center,uiScale};
})();
