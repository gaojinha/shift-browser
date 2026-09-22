import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import AppIcon from './components/AppIcon.vue'
import './styles/main.css'

const app = createApp(App)
app.use(createPinia())
app.component('AppIcon', AppIcon)
app.mount('#app')
