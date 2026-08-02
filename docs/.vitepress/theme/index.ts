import { h } from 'vue'
import DefaultTheme from 'vitepress/theme'
import HomeInstall from './HomeInstall.vue'
import ReleaseStamp from './ReleaseStamp.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      'home-hero-info-after': () => h(HomeInstall)
    }),
  enhanceApp({ app }) {
    app.component('ReleaseStamp', ReleaseStamp)
  }
}
