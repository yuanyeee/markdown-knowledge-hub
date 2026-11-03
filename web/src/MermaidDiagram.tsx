import {useEffect,useState} from 'react';

let nextDiagram=0;
const engine=()=>import('mermaid').then(({default: mermaid})=>{
  mermaid.initialize({startOnLoad:false,securityLevel:'strict',htmlLabels:false,maxTextSize:30000,maxEdges:500});
  return mermaid;
});
let ready:ReturnType<typeof engine>|undefined;

export function MermaidDiagram({source}:{source:string}) {
  const [image,setImage]=useState('');
  const [error,setError]=useState(false);
  const [zoom,setZoom]=useState(1);
  useEffect(()=>{
    let active=true;
    let url='';
    setImage('');setError(false);setZoom(1);
    async function render(){
      try{
        if(source.length>30000||/^\s*---/u.test(source)||/%%\s*\{/u.test(source))throw new Error('Unsupported diagram configuration');
        const mermaid=await (ready??=engine());
        if(!active)return;
        const {svg}=await mermaid.render(`wiki-diagram-${++nextDiagram}`,source);
        if(!active)return;
        url=URL.createObjectURL(new Blob([svg],{type:'image/svg+xml'}));
        setImage(url);
      }catch{if(active)setError(true);}
    }
    void render();
    return()=>{active=false;if(url)URL.revokeObjectURL(url);};
  },[source]);
  return <figure className="mermaid-diagram">
    {image?<><button type="button" onClick={()=>setZoom(z=>z===1?1.75:1)}>{zoom===1?'図を拡大':'元の倍率'}</button>
      <div className="mermaid-scroll"><img src={image} alt="処理フロー図" style={{width:`${zoom*100}%`}}/></div></>
      :error?<p role="status">図を表示できません。下の定義または処理表をご確認ください。</p>:<p>図を描画しています…</p>}
    <details><summary>Mermaid定義</summary><pre>{source}</pre></details>
  </figure>;
}
