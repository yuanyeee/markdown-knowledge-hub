import {useEffect,useRef,useState} from 'react';

let nextDiagram=0;
const engine=()=>import('mermaid').then(({default: mermaid})=>{
  mermaid.initialize({startOnLoad:false,securityLevel:'strict',htmlLabels:false,maxTextSize:30000,maxEdges:500});
  return mermaid;
});
let ready:ReturnType<typeof engine>|undefined;
type Diagram={url:string;width:number;height:number};

function DiagramViewport({image,full,onToggle}:{image:Diagram;full:boolean;onToggle:()=>void}) {
  const viewport=useRef<HTMLDivElement>(null);
  const [size,setSize]=useState({width:0,height:0});
  const [mode,setMode]=useState<number|'fit'>(1);
  useEffect(()=>{
    const element=viewport.current;
    if(!element)return;
    const observer=new ResizeObserver(()=>{
      const width=element.clientWidth;
      const height=element.clientHeight;
      setSize(previous=>previous.width===width&&previous.height===height?previous:{width,height});
    });
    observer.observe(element);
    return ()=>observer.disconnect();
  },[]);
  const zoom=mode==='fit'&&size.width&&size.height
    ? Math.min(1,(size.width-16)/image.width,(size.height-16)/image.height)
    : typeof mode==='number'?mode:1;
  const adjust=(delta:number)=>setMode(Math.max(.1,Math.min(4,Math.round((zoom+delta)*100)/100)));
  return <div className="mermaid-viewer">
    <div className="mermaid-toolbar" aria-label="図の表示操作">
      <button type="button" aria-label="図を縮小" onClick={()=>adjust(-.2)}>−</button>
      <output aria-label="図の倍率">{Math.round(zoom*100)}%</output>
      <button type="button" aria-label="図を拡大" onClick={()=>adjust(.2)}>＋</button>
      <button type="button" onClick={()=>setMode(1)}>100%</button>
      <button type="button" aria-pressed={mode==='fit'} onClick={()=>{viewport.current?.scrollTo(0,0);setMode('fit');}}>全体を表示</button>
      <button type="button" onClick={onToggle}>{full?'図を閉じる（Esc）':'図を全画面で開く'}</button>
    </div>
    <div className="mermaid-scroll" ref={viewport} tabIndex={0} aria-label="処理フロー図のスクロール領域"
      style={full?undefined:{height:Math.min(440,image.height+24)}}>
      <img src={image.url} alt="処理フロー図" style={{width:image.width*zoom,height:image.height*zoom}}/>
    </div>
  </div>;
}

function DiagramDialog({image,onClose}:{image:Diagram;onClose:()=>void}) {
  const dialog=useRef<HTMLDialogElement>(null);
  useEffect(()=>{
    const element=dialog.current;
    element?.showModal();
    return ()=>element?.close();
  },[]);
  return <dialog className="mermaid-fullscreen" aria-label="処理フロー図の全画面表示" ref={dialog}
    onKeyDown={event=>{if(event.key==='Escape')event.stopPropagation();}}
    onCancel={event=>{event.preventDefault();onClose();}}>
    <DiagramViewport image={image} full onToggle={onClose}/>
  </dialog>;
}

export function MermaidDiagram({source}:{source:string}) {
  const [image,setImage]=useState<Diagram|null>(null);
  const [error,setError]=useState(false);
  const [full,setFull]=useState(false);
  useEffect(()=>{
    let active=true;
    let url='';
    setImage(null);setError(false);setFull(false);
    async function render(){
      try{
        if(source.length>30000||/^\s*---/u.test(source)||/%%\s*\{/u.test(source))throw new Error('Unsupported diagram configuration');
        const mermaid=await (ready??=engine());
        if(!active)return;
        const {svg}=await mermaid.render(`wiki-diagram-${++nextDiagram}`,source);
        if(!active)return;
        const root=new DOMParser().parseFromString(svg,'image/svg+xml').documentElement;
        const [, , width, height]=(root.getAttribute('viewBox')??'').trim().split(/[\s,]+/).map(Number);
        if(!Number.isFinite(width)||!Number.isFinite(height)||width<=0||height<=0)throw new Error('Invalid diagram dimensions');
        url=URL.createObjectURL(new Blob([svg],{type:'image/svg+xml'}));
        setImage({url,width,height});
      }catch{if(active)setError(true);}
    }
    void render();
    return()=>{active=false;if(url)URL.revokeObjectURL(url);};
  },[source]);
  return <figure className="mermaid-diagram">
    {image?<><DiagramViewport key={image.url} image={image} full={false} onToggle={()=>setFull(true)}/>
      {full&&<DiagramDialog image={image} onClose={()=>setFull(false)}/>}</>
      :error?<p role="status">図を表示できません。下の定義または処理表をご確認ください。</p>:<p>図を描画しています…</p>}
    <details><summary>Mermaid定義</summary><pre>{source}</pre></details>
  </figure>;
}
