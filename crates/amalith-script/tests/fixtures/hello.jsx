#target illustrator

$.writeln("hi from amalith-script");
$.writeln("fileName: " + $.fileName);

var f = new File("/tmp/amalith-script-does-not-exist.txt");
$.writeln("exists: " + f.exists);

var folder = new Folder(".");
$.writeln("cwd exists: " + folder.exists);

$.global.marker = "set-by-hello";
$.writeln("global marker: " + $.global.marker);

app.userInteractionLevel = UserInteractionLevel.DONTDISPLAYALERTS;
$.writeln("done");
