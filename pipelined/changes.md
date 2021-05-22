* Remove AppBuilder
* Add SubApps
* `Res<Box<dyn RenderResourceContext>>` -> `Res<RenderResources>`
* Removed RenderResourceBindings
* Make shaders and pipelines proper render resources (removes dependency on bevy_asset and is generally a cleaner api)
* Removed RenderResources / RenderResource traits


TODO

* remove renderresourcebindings
* remove asset tracking from render resources