import {zipSync,strToU8} from 'fflate';
export type PackageAttachment={id:number;filename?:string};
export const safePackageFilename=(name:string)=>name.replace(/[\\/\x00-\x1f:*?"<>|]/g,'_').replace(/^\.+/,'_').slice(0,180)||'附件';
/** Each fetch goes through the existing authorized attachment download endpoint. */
export async function makeHandoffPackage(html:string,attachments:PackageAttachment[],fetchAttachment:(id:number)=>Promise<{bytes:ArrayBuffer;filename?:string}>,missing:string[]){
 const files:Record<string,Uint8Array>={'申請表與計算明細.html':strToU8(html)};
 const report=['本包為可審核交接資料，尚未送出 EasyFlow，未執行付款。','原始 PDF 可能包含多頁，請依表單來源頁碼核對。',''];
 const failures:string[]=[];
 for(const a of [...new Map(attachments.map(x=>[x.id,x])).values()]){
  try{const file=await fetchAttachment(a.id);const name=`附件/${a.id}-${safePackageFilename(file.filename||a.filename||`附件-${a.id}`)}`;files[name]=new Uint8Array(file.bytes);report.push(`已包含：${name}`);}
  catch{failures.push(`附件 #${a.id} 下載失敗，未包含於本包`);}
 }
 report.push('','缺項與待確認：',...missing,...failures);
 files['附件與缺件清單.txt']=strToU8(report.join('\r\n'));
 return {bytes:zipSync(files,{level:0}),failures};
}
