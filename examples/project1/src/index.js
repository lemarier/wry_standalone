console.log(`This is a console from JAVASCRIPT ! :D`);

const webview = new Webview("./test.html");

console.log(webview);

webview.run(({event}) => {
   switch (event) {
     case 'close':
       Deno.exit()
       break;
     case 'windowCreated':
       console.log("It works! Window created , if webview didn't show, try to resize window");
       break;
     case 'domContentLoaded':
       console.log("It works! domContentLoaded")
       break;
     }
 }, 16);