console.log(`This is a console from JAVASCRIPT ! :D`);
class Webview {
   constructor(url) {
      this.id = Deno.core.jsonOpSync('wry_new', { url });
   }

   loop() {
      return Deno.core.jsonOpSync('wry_loop', { id: this.id }) === false;
   }

   step() {
      return Deno.core.jsonOpSync('wry_step', { id: this.id });
   }

   run(
      callback,
      delta = 1,
    ) {
      return new Promise((resolve) => {
        const interval = setInterval(() => {
          const success = this.loop();
  
          if (callback !== undefined) {
            const events = this.step();
  
            for (const event of events) {
              callback(event);
            }
          }
  
          if (!success) {
            resolve();
            clearInterval(interval);
          }
        }, delta);
      });
    }
}

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
 });