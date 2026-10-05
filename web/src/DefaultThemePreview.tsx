import { useId } from "react";

export function DefaultThemePreview() {
  const id = useId().replaceAll(":", "");
  return <svg viewBox="0 0 360 245" role="img" aria-label="默认主题预览" className="default-theme-preview">
    <defs>
      <linearGradient id={`${id}-background`} x2="1" y2="1"><stop stopColor="#eef4f5"/><stop offset=".55" stopColor="#e8f5ee"/><stop offset="1" stopColor="#f5f5f8"/></linearGradient>
      <radialGradient id={`${id}-globe`} cx=".35" cy=".3"><stop stopColor="#fafcfd"/><stop offset=".75" stopColor="#d9e4df"/><stop offset="1" stopColor="#94b6a8"/></radialGradient>
      <clipPath id={`${id}-clip`}><circle cx="259" cy="117" r="81"/></clipPath>
      <pattern id={`${id}-dots`} width="5" height="5" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r=".8" fill="#779d8d"/></pattern>
    </defs>
    <rect width="360" height="245" fill={`url(#${id}-background)`}/>
    <path d="M0 67L360 0M0 127L360 50M0 187L360 110M0 245L360 170" stroke="white" opacity=".45"/>
    <circle cx="24" cy="22" r="9" fill="#27272a"/><path d="M17 22h4l2-4 3 8 2-4h3" stroke="white" fill="none" strokeWidth="1.3"/>
    <text x="40" y="26" fill="#27272a" fontSize="11" fontWeight="600">Monitor</text>
    <circle cx="259" cy="117" r="83" fill="white" opacity=".75"/>
    <circle cx="259" cy="117" r="81" fill={`url(#${id}-globe)`}/>
    <g clipPath={`url(#${id}-clip)`} stroke="#85a496" strokeWidth=".5" opacity=".45" fill="none">
      {[24,43,62].map(r=><ellipse key={r} cx="259" cy="117" rx={r} ry="81"/>)}
      {[25,47,65].map(r=><ellipse key={r} cx="259" cy="117" rx="81" ry={r}/>)}
      <path fill={`url(#${id}-dots)`} stroke="none" d="M205 44l28 10 11 22-16 20 10 28-16 6-23-23-17-20zm62 12 30-17 44 19-3 34-22 12-20-8-14 18-24-25zm-9 63 28-2 18 19-10 40-17 12-11-22-11-18zm55 59 20-8 9 21-17 9-15-9z"/>
    </g>
    <circle cx="280" cy="139" r="3" fill="#21a779"/><circle cx="280" cy="139" r="7" fill="none" stroke="#21a779" opacity=".25"/>
    <g fill="white" opacity=".87">
      <rect x="19" y="62" width="132" height="47" rx="6"/>
      <rect x="19" y="119" width="132" height="95" rx="7"/>
      <rect x="162" y="205" width="178" height="24" rx="5"/>
    </g>
    <g fill="#bac4bf"><rect x="30" y="73" width="39" height="4" rx="2"/><rect x="83" y="73" width="35" height="4" rx="2"/><rect x="30" y="86" width="27" height="9" rx="2"/><rect x="83" y="86" width="25" height="9" rx="2"/><rect x="45" y="133" width="65" height="5" rx="2"/><rect x="30" y="154" width="43" height="3" rx="1"/><rect x="30" y="180" width="35" height="3" rx="1"/><rect x="30" y="194" width="91" height="3" rx="1"/></g>
    <circle cx="33" cy="135" r="3" fill="#28a878"/>
    <rect x="30" y="164" width="110" height="3" rx="1.5" fill="#e4e8e6"/><rect x="30" y="164" width="38" height="3" rx="1.5" fill="#32af82"/>
  </svg>;
}
