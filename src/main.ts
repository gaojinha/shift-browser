import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import AppIcon from './components/AppIcon.vue'
import { i18n } from './i18n'
import './styles/main.css'

const app = createApp(App)
app.use(createPinia())
app.use(i18n)
app.component('AppIcon', AppIcon)
app.mount('#app')
