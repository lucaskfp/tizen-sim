// The TV's screen size, as `screen.width`/`height` (TV
// guides/fundamentals/managing-screen-resolution.html names them as how an
// App learns the resolution). The Engine's metrics override already reports
// it where the Engine can; old ones (M47) report the desktop's monitor, so the
// Shims set it on every Engine.
(function () {
  var size: { [property: string]: number } = {
    width: config.screen.width,
    height: config.screen.height,
    availWidth: config.screen.width,
    availHeight: config.screen.height,
  };
  for (var property in size) {
    try {
      Object.defineProperty(window.screen, property, {
        value: size[property],
        configurable: true,
      });
    } catch (e) {
      // An Engine where `screen` can't be changed keeps its own values.
    }
  }
})();
