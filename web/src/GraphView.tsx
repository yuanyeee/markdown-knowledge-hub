import { useEffect,useRef,useState } from 'react';
import Graph from 'graphology';
import Sigma from 'sigma';
import { layoutNodes, type GraphNode } from './graph';
import { read } from './transport';
type Data={nodes:GraphNode[];edges:{source:string;target:string}[];truncated:boolean};
export function GraphView({project,center,onOpen}:{project:string;center?:string;onOpen:(path:string)=>void}) {
  const [data,setData]=useState<Data|null>(null);
  const [error,setError]=useState('');
  const [summary,setSummary]=useState(false);
  const container=useRef<HTMLDivElement>(null);
  const open=useRef(onOpen); open.current=onOpen;
  useEffect(()=>{
    let active=true; setData(null); setError('');
    read<Data>(summary?'graph-summary':'graph',project,{path:center||''}).then(v=>{if(active)setData(v);}).catch(e=>{if(active)setError(String(e));});
    return()=>{active=false;};
  },[project,center,summary]);
  useEffect(()=>{
    if(!data?.nodes.length||!container.current)return;
    let renderer:Sigma|undefined;
    try {
      const graph=new Graph({type:'directed'});
      for(const n of layoutNodes(data.nodes))graph.addNode(n.path,{x:n.x,y:n.y,label:n.title,size:n.path===center?14:8,forceLabel:(summary&&data.nodes.length<30)||n.path===center,color:n.path===center?'#c18b2e':'#327d68'});
      for(const e of data.edges)if(graph.hasNode(e.source)&&graph.hasNode(e.target)&&!graph.hasDirectedEdge(e.source,e.target))graph.addDirectedEdge(e.source,e.target,{size:1.5,color:'#b4c7c0'});
      renderer=new Sigma(graph,container.current,{renderEdgeLabels:false,defaultEdgeType:'arrow',labelFont:'Meiryo, sans-serif',stagePadding:100});
      if(!summary)renderer.on('clickNode',({node})=>open.current(node));
    } catch(e) { setError(`図を描画できませんでした。下の文書一覧から参照できます。${String(e)}`); }
    return()=>{renderer?.kill();};
  },[data,center,summary]);
  return <section aria-label="リンク図"><button onClick={()=>setSummary(v=>!v)}>{summary?'文書単位の図':'フォルダー集約図'}</button><h2>{summary?'フォルダー集約図':center?'選択文書のリンク図':'プロジェクトのリンク図'}</h2><p className="hint">{summary?'全体の文書を先頭フォルダー別に集約しています。':'ノードをクリックするとWikiを開きます。'}ドラッグ・ホイールで移動／拡大できます。</p>{error&&<p role="alert">{error}</p>}{!data&&!error&&<p>リンク図を読み込んでいます…</p>}<div className="graph-canvas" ref={container}/>{data&&<><p>{data.nodes.length}{summary?'グループ':'文書'}・{data.edges.length}{summary?'集約リンク':'リンク'}{data.truncated?'（上限により一部を省略）':''}</p>{!data.nodes.length&&<p>Markdown文書がありません。</p>}<details><summary>図に含まれる文書</summary>{data.nodes.map(n=><button className="hit" key={n.path} disabled={summary} onClick={()=>onOpen(n.path)}>{n.title} <small>{n.path}</small></button>)}</details></>}</section>;
}
