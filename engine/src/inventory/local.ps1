$records=@(foreach($r in Get-LocalRegistrations) {
    $fontOnly=$null;$libraryOnly=$null
    if($r.ProductCode -and ('ReleaseScoutMsi' -as [type])) {$fontOnly=[ReleaseScoutMsi]::FontOnly($r.ProductCode);$libraryOnly=[ReleaseScoutMsi]::LibraryOnly($r.ProductCode)}
    [pscustomobject]@{Id=$r.RegistrationId;Name=$r.DisplayName;Version=$r.DisplayVersion;
        Scope=$r.Scope;InstallLocation=$r.InstallLocation;IconPath=(Get-RegisteredIcon $r.DisplayIcon);
        metadata=@{registrationId=$r.RegistrationId;publisher=$r.Publisher;description=$r.Comments;homepage=$r.Homepage;
            systemComponent=if($null -ne $r.SystemComponent){[bool]$r.SystemComponent}else{$null};
            parentPackage=$r.ParentKeyName;releaseType=$r.ReleaseType;fontOnly=$fontOnly;libraryOnly=$libraryOnly}}
})
@{installed=$records}|ConvertTo-Json -Depth 8 -Compress
