import { useEffect,useRef,useState } from 'react';
import Graph from 'graphology';
import Sigma from 'sigma';
import { layoutNodes, type GraphNode } from './graph';
import { read } from './transport';
type Data={nodes:GraphNode[];edges:{source:string;target:string;call?:boolean}[];truncated:boolean};
export function GraphView({project,center,expanded,onToggleExpand,onOpen}:{project:string;center?:string;expanded:boolean;onToggleExpand:()=>void;onOpen:(path:string)=>void}) {
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
    let observer:ResizeObserver|undefined;
    try {
      const graph=new Graph({type:'directed'});
      for(const n of layoutNodes(data.nodes))graph.addNode(n.path,{x:n.x,y:n.y,label:n.title,size:n.path===center?14:8,forceLabel:(summary&&data.nodes.length<30)||n.path===center,color:n.path===center?'#c18b2e':'#327d68'});
      for(const e of data.edges)if(graph.hasNode(e.source)&&graph.hasNode(e.target)&&!graph.hasDirectedEdge(e.source,e.target))graph.addDirectedEdge(e.source,e.target,{size:e.call?3:1.5,color:e.source===center?'#246bb0':e.target===center?'#b35d21':'#b4c7c0'});
      renderer=new Sigma(graph,container.current,{renderEdgeLabels:false,defaultEdgeType:'arrow',labelFont:'Meiryo, sans-serif',stagePadding:100});
      const host=container.current;
      observer=new ResizeObserver(()=>{
        if(host.clientWidth&&host.clientHeight){
          renderer?.resize();
          renderer?.scheduleRender();
        }
      });
      observer.observe(host);
      if(!summary)renderer.on('clickNode',({node})=>open.current(node));
    } catch(e) { setError(`図を描画できませんでした。下の文書一覧から参照できます。${String(e)}`); }
    return()=>{observer?.disconnect();renderer?.kill();};
  },[data,center,summary]);
  return <section className="link-graph" aria-label="リンク図"><div className="link-graph-controls"><button onClick={()=>setSummary(v=>!v)}>{summary?'文書単位の図':'フォルダー集約図'}</button><button type="button" aria-pressed={expanded} onClick={onToggleExpand}>{expanded?'リンク図を元に戻す（Esc）':'リンク図を拡大'}</button></div><h2>{summary?'フォルダー集約図':center?'選択文書のリンク図':'プロジェクトのリンク図'}</h2><p className="hint">{summary?'全体の文書を先頭フォルダー別に集約しています。':'橙色は選択文書への参照、青色は選択文書からの参照です。太線は処理フローで確認した呼び出しです。矢印は参照先を指します。ノードからWikiを開けます。'}ドラッグ・ホイールで移動／拡大できます。</p>{error&&<p role="alert">{error}</p>}{!data&&!error&&<p>リンク図を読み込んでいます…</p>}<div className="graph-canvas" ref={container}/>{data&&<><p>{data.nodes.length}{summary?'グループ':'文書'}・{data.edges.length}{summary?'集約リンク':'リンク'}{data.truncated?'（上限により一部を省略）':''}</p>{!data.nodes.length&&<p>Markdown文書がありません。</p>}<details><summary>図に含まれる文書</summary>{data.nodes.map(n=><button className="hit" key={n.path} disabled={summary} onClick={()=>onOpen(n.path)}>{n.title} <small>{n.path}</small></button>)}</details></>}</section>;
}
