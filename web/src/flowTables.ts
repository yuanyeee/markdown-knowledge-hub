type Node = {type:string; value?:string; lang?:string; children?:Node[]};

const text=(node:Node):string=>node.value??node.children?.map(text).join('')??'';

export function remarkFlowTables(){
  return (tree:Node)=>{
    const result:Node[]=[];
    let hasDiagram=false;
    for(const node of tree.children??[]){
      if(node.type==='heading')hasDiagram=false;
      if(node.type==='code'&&node.lang==='mermaid')hasDiagram=true;
      const [header,...rows]=node.children??[];
      if(node.type==='table'&&!hasDiagram&&header&&rows.length>0&&rows.length<=150){
        const columns=header.children?.map(cell=>text(cell).trim())??[];
        const id=columns.indexOf('#'), label=columns.indexOf('処理'), next=columns.indexOf('次へ');
        if(id>=0&&label>=0&&next>=0){
          const entries=rows.map(row=>row.children?.map(cell=>text(cell).trim())??[]);
          const ids=new Set(entries.map(row=>row[id]));
          const edges=entries.flatMap(row=>[...(row[next]??'').matchAll(/(?:→|->)\s*(\d+)/gu)]
            .filter(match=>ids.has(match[1])).map(match=>`n${row[id]} --> n${match[1]}`));
          if(ids.size===entries.length&&entries.every(row=>/^\d+$/u.test(row[id]))&&edges.length){
            const nodes=entries.map(row=>`n${row[id]}["${row[id]}. ${(row[label]??'').replace(/[^\p{L}\p{N} ._()-]/gu,' ').slice(0,100)}"]`);
            result.push({type:'code',lang:'mermaid',value:['flowchart TD',...nodes,...edges].join('\n')});
            hasDiagram=true;
          }
        }
      }
      result.push(node);
    }
    tree.children=result;
  };
}
