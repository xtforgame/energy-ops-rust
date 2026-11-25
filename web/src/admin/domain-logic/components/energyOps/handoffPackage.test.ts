import {it,expect} from 'vitest';
import {unzipSync,strFromU8} from 'fflate';
import {makeHandoffPackage} from './handoffPackage';
it('includes each attachment once, sanitizes paths, and states actual failures',async()=>{
 const r=await makeHandoffPackage('<h1>預覽</h1>',[{id:1},{id:1},{id:2}],async id=>{if(id===2)throw Error('denied');return {bytes:new Uint8Array([37,80,68,70]).buffer,filename:'../../原單.pdf'};},['完整租約待補']);
 const files=unzipSync(r.bytes);expect(Object.keys(files)).toHaveLength(3);
 const manifest=strFromU8(files['附件與缺件清單.txt']);expect(manifest).toContain('完整租約待補');expect(manifest).toContain('附件 #2 下載失敗');
 expect(Object.keys(files).filter(x=>x.startsWith('附件/'))[0]).not.toContain('../');expect(r.failures).toHaveLength(1);
});
