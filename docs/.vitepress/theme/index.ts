import DefaultTheme from 'vitepress/theme'
import ReleaseStamp from './ReleaseStamp.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('ReleaseStamp', ReleaseStamp)
  }
}
