function Test-ConsoleExecutable($path) {
    $reader=$null
    try {
        $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
        $reader=[IO.BinaryReader]::new($stream)
        if($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) { return $null }
        [void]$stream.Seek(0x3c,[IO.SeekOrigin]::Begin)
        $pe=$reader.ReadUInt32()
        if($pe -lt 64 -or $pe -gt 1MB -or $stream.Length -lt $pe+94) { return $null }
        [void]$stream.Seek($pe,[IO.SeekOrigin]::Begin)
        if($reader.ReadUInt32() -ne 0x4550) { return $null }
        [void]$stream.Seek($pe+20,[IO.SeekOrigin]::Begin)
        if($reader.ReadUInt16() -lt 70 -or ($reader.ReadUInt16() -band 0x2000)) { return $null }
        if($reader.ReadUInt16() -notin @(0x10b,0x20b)) { return $null }
        [void]$stream.Seek($pe+24+68,[IO.SeekOrigin]::Begin)
        $subsystem=$reader.ReadUInt16()
        if($subsystem -eq 3) { return $true }
        if($subsystem -eq 2) { return $false }
    } catch {} finally { if($reader){$reader.Dispose()} }
    return $null
}
