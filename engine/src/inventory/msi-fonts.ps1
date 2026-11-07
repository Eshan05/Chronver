try { Add-Type -ErrorAction Stop -TypeDefinition @'
using System;
using System.IO;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class ReleaseScoutMsi {
    [DllImport("msi.dll",CharSet=CharSet.Unicode)] static extern uint MsiGetProductInfo(string product,string property,StringBuilder value,ref uint size);
    [DllImport("msi.dll",CharSet=CharSet.Unicode)] static extern uint MsiOpenDatabase(string path,IntPtr mode,out uint database);
    [DllImport("msi.dll",CharSet=CharSet.Unicode)] static extern uint MsiDatabaseOpenView(uint database,string sql,out uint view);
    [DllImport("msi.dll")] static extern uint MsiViewExecute(uint view,uint record);
    [DllImport("msi.dll")] static extern uint MsiViewFetch(uint view,out uint record);
    [DllImport("msi.dll",CharSet=CharSet.Unicode)] static extern uint MsiRecordGetString(uint record,uint field,StringBuilder value,ref uint size);
    [DllImport("msi.dll")] static extern uint MsiCloseHandle(uint handle);
    static List<string> Read(uint db,string sql) {
        uint view;
        if(MsiDatabaseOpenView(db,sql,out view)!=0) return null;
        try {
            if(MsiViewExecute(view,0)!=0) return null;
            var rows=new List<string>();
            while(true) {
                uint row;var status=MsiViewFetch(view,out row);
                if(status==259) return rows;
                if(status!=0) return null;
                try {
                    uint size=2048;var value=new StringBuilder((int)size);
                    if(MsiRecordGetString(row,1,value,ref size)!=0) return null;
                    rows.Add(value.ToString());
                    if(rows.Count>1000) return null;
                } finally {MsiCloseHandle(row);}
            }
        } finally {MsiCloseHandle(view);}
    }
    public static bool? FontOnly(string product) {
        uint size=2048;var path=new StringBuilder((int)size);
        if(MsiGetProductInfo(product,"LocalPackage",path,ref size)!=0) return null;
        return Inspect(path.ToString(),product);
    }
    public static bool? LibraryOnly(string product) {
        uint size=2048;var path=new StringBuilder((int)size);
        if(MsiGetProductInfo(product,"LocalPackage",path,ref size)!=0) return null;
        try {
            var file=new FileInfo(path.ToString());
            if(!file.Exists || file.Length>64*1024*1024 || (file.Attributes&FileAttributes.ReparsePoint)!=0) return null;
            uint db;if(MsiOpenDatabase(file.FullName,IntPtr.Zero,out db)!=0) return null;
            try {
                var identity=Read(db,"SELECT `Value` FROM `Property` WHERE `Property`='ProductCode'");
                if(identity==null || identity.Count!=1 || !String.Equals(identity[0],product,StringComparison.OrdinalIgnoreCase)) return null;
                var tables=Read(db,"SELECT `Name` FROM `_Tables`");
                if(tables==null || !tables.Contains("File")) return null;
                if(tables.Contains("Shortcut")) {var shortcuts=Read(db,"SELECT `Shortcut` FROM `Shortcut`");if(shortcuts==null)return null;if(shortcuts.Count>0)return false;}
                var files=Read(db,"SELECT `FileName` FROM `File`");if(files==null || files.Count==0)return null;
                bool dll=false;
                foreach(var name in files) {
                    var parts=name.Split('|');var ext=Path.GetExtension(parts[parts.Length-1]).ToLowerInvariant();
                    if(ext==".dll") {dll=true;continue;}
                    if(ext!=".manifest" && ext!=".cat" && ext!=".config" && ext!=".txt" && ext!=".rtf")return false;
                }
                return dll;
            } finally {MsiCloseHandle(db);}
        } catch {return null;}
    }
    public static bool? Inspect(string path,string product) {
        try {
            var file=new FileInfo(path);
            if(!file.Exists || file.Length>64*1024*1024 || (file.Attributes&FileAttributes.ReparsePoint)!=0) return null;
            uint db;
            // MSIDBOPEN_READONLY. No product configuration, actions or repair queries.
            if(MsiOpenDatabase(path,IntPtr.Zero,out db)!=0) return null;
            try {
                var identity=Read(db,"SELECT `Value` FROM `Property` WHERE `Property`='ProductCode'");
                if(identity==null || identity.Count!=1 || !String.Equals(identity[0],product,StringComparison.OrdinalIgnoreCase)) return null;
                var tables=Read(db,"SELECT `Name` FROM `_Tables`");
                if(tables==null) return null;
                if(!tables.Contains("Font")) return false;
                var fonts=Read(db,"SELECT `File_` FROM `Font`");
                var files=Read(db,"SELECT `File` FROM `File`");
                if(fonts==null || files==null || fonts.Count==0 || files.Count==0) return null;
                var fontIds=new HashSet<string>(fonts,StringComparer.Ordinal);
                foreach(var id in files) if(!fontIds.Contains(id)) return false;
                if(tables.Contains("Shortcut")) {
                    var shortcuts=Read(db,"SELECT `Shortcut` FROM `Shortcut`");
                    if(shortcuts==null) return null;
                    if(shortcuts.Count>0) return false;
                }
                return true;
            } finally {MsiCloseHandle(db);}
        } catch {return null;}
    }
}
'@
} catch {}
